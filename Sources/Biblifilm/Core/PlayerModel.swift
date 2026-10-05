import AppKit
import Combine
import Foundation
import IOKit.pwr_mgt
import VLCKit

/// The in-app player: owns the VLC engine, exposes playback state to SwiftUI,
/// and keeps the watch history up to date.
@MainActor
final class PlayerModel: NSObject, ObservableObject {
    static let shared = PlayerModel()

    /// The item on screen. Non-nil means the player is showing.
    @Published private(set) var current: MediaItem?
    @Published private(set) var isPlaying = false {
        didSet { updateSleepAssertion() }
    }
    @Published private(set) var isBuffering = false
    @Published private(set) var time: Double = 0        // seconds
    @Published private(set) var duration: Double = 0    // seconds
    @Published var volume: Int = 100 { didSet { applyVolume() } }
    @Published private(set) var failure: String?

    /// Audio and subtitle tracks of the current file. VLC only knows them
    /// once it has parsed the stream, so these fill in shortly after play().
    @Published private(set) var audioTracks: [MediaTrack] = []
    @Published private(set) var subtitleTracks: [MediaTrack] = []
    @Published private(set) var currentAudioTrack: Int = -1
    @Published private(set) var currentSubtitleTrack: Int = -1

    /// Transient on-screen readout after a volume gesture.
    @Published private(set) var volumeHUD: Int?

    /// Transient on-screen readout after a track change.
    @Published private(set) var infoHUD: String?

    /// Remaining items, used to roll on to the next episode.
    private var queue: [MediaItem] = []

    /// One selectable audio or subtitle track. `id` is VLC's track id;
    /// -1 means "no subtitles".
    struct MediaTrack: Identifiable, Hashable {
        let id: Int
        let name: String
    }

    /// What to pick when a file's tracks show up. VLC renumbers tracks per
    /// file, so a choice is remembered by label ("Track 2 - [French]") and
    /// re-applied to the next episode.
    private enum TrackChoice: Equatable {
        case automatic       // leave VLC's own pick alone
        case off             // subtitles only
        case named(String)
    }

    private var stickyAudio: TrackChoice = .automatic
    private var stickySubtitle: TrackChoice = .automatic
    private var pendingAudio: TrackChoice = .automatic
    private var pendingSubtitle: TrackChoice = .automatic
    /// When the current file's track list first showed up. Remembered
    /// choices are re-applied for a few seconds from then on.
    private var tracksSeenAt: Date?
    private static let trackSettleTime: TimeInterval = 5

    /// Replaced on every load -- see `freshEngine()`.
    private(set) var player = VLCMediaPlayer()
    private var hudTask: Task<Void, Never>?
    private var infoTask: Task<Void, Never>?
    private var openWatchdog: Task<Void, Never>?
    private var stateTicker: Task<Void, Never>?
    private var lastPersist: Date = .distantPast
    private var lastPersistedPosition: Double = -1
    private var pendingResume: Double?
    /// True once real playback has been observed for the current file.
    /// Guards against a file that fails to open reporting "ended" instantly.
    private var hasPlayed = false
    /// Set when the viewer pauses on purpose, so a pause on the last frame
    /// is not read as the end of the episode.
    private var pausedByUser = false

    /// Stall detection. The engine clock as of the last tick, and when it
    /// last moved: a clock standing still while nobody asked for a pause is
    /// the only honest sign that playback has died.
    private var lastEngineTime: Int32 = -1
    private var lastProgressAt: Date = .distantPast
    /// Reloads tried for the current stall, and when the last one happened.
    private var recoveryAttempts = 0
    private var lastRecoveryAt: Date = .distantPast
    /// When we last stopped the engine ourselves, so our own `.stopped`
    /// notifications are not mistaken for the player dying under us.
    private var lastDeliberateStop: Date = .distantPast
    /// Running while we sit out a drive that has gone away.
    private var awaitingVolume: Task<Void, Never>?
    /// Waits for the displays to stop changing before rebuilding the picture.
    private var displaySettle: Task<Void, Never>?
    /// Set when the displays changed during a pause. Reloading then would
    /// mean playing and pausing again before VLC has shown a frame -- a black
    /// picture under a spinner -- so the rebuild waits for the next "play".
    private var rebuildOnResume = false

    /// How long the clock may stand still before it counts as a freeze. Long
    /// enough not to fire on a slow seek, short enough that nobody has
    /// reached for the mouse yet.
    private static let stallTimeout: TimeInterval = 6
    /// Reloads in quick succession before we stop trying and say so.
    private static let maxRecoveryAttempts = 3
    /// How long to wait for a drive that has dropped off. The one this was
    /// written for comes back in about five seconds; a minute and a half is
    /// generous enough to cover a bad one and short enough that a drive
    /// genuinely unplugged does not leave a spinner up all evening.
    private static let volumeWaitTimeout: TimeInterval = 90
    /// Quiet time required after the last display change before rebuilding.
    /// A screen renegotiating its link fires ten changes a second, sometimes
    /// for over a minute; rebuilding on each one would only make it worse.
    private static let displaySettleDelay: UInt64 = 2_000_000_000

    private override init() {
        super.init()
        player.delegate = self
        volume = Int(player.audio?.volume ?? 100)
        DisplayWatcher.start { PlayerModel.shared.displaysChanged() }
    }

    /// `isPlaying` used to be driven only by state notifications (unreliable)
    /// and time ticks (which stop while paused). A stale `true` made the pause
    /// button call `pause()` again -- which does nothing -- so playback never
    /// resumed. Polling keeps the flag honest.
    private func startStateTicker() {
        stateTicker?.cancel()
        stateTicker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 400_000_000)
                guard let self, self.current != nil else { return }
                let live = self.player.isPlaying
                if live != self.isPlaying { self.isPlaying = live }
                if live && self.isBuffering { self.isBuffering = false }
                self.refreshTracks()
                self.checkForEndOfFile()
                self.checkForStall()
                self.persistProgress()
            }
        }
    }

    var isPresenting: Bool { current != nil }

    var canPlayNext: Bool { !queue.isEmpty }

    // MARK: - Opening

    /// Show the player for `item`, queueing `rest` to follow it.
    func open(_ item: MediaItem, following rest: [MediaItem] = [], fromStart: Bool = false) {
        failure = nil
        queue = rest
        start(item, fromStart: fromStart)
    }

    private func start(_ item: MediaItem, fromStart: Bool) {
        persistProgress(force: true)

        current = item
        time = 0
        duration = 0
        lastPersistedPosition = -1
        recoveryAttempts = 0
        lastRecoveryAt = .distantPast
        startStateTicker()

        load(item, resumingAt: fromStart ? nil : WatchStore.shared.resumePosition(for: item))
    }

    /// Hand a file to VLC and reset everything that belongs to one playback.
    /// Used both to open a film and to pick it back up after a freeze, which
    /// is the same work: the engine keeps nothing useful across a reload.
    private func load(_ item: MediaItem, resumingAt resume: Double?) {
        awaitingVolume?.cancel()
        awaitingVolume = nil
        lastDeliberateStop = .now
        freshEngine()

        isBuffering = true
        hasPlayed = false
        pausedByUser = false
        rebuildOnResume = false
        lastEngineTime = -1
        lastProgressAt = .now
        audioTracks = []
        subtitleTracks = []
        currentAudioTrack = -1
        currentSubtitleTrack = -1
        pendingAudio = Self.savedAudio(for: item).map { .named($0) } ?? stickyAudio
        pendingSubtitle = Self.savedSubtitle(for: item) ?? stickySubtitle
        tracksSeenAt = nil

        player.media = VLCMedia(url: item.url)
        pendingResume = resume

        // The video surface only exists once `current` is set: SwiftUI builds
        // PlayerScreen in reaction to it, so on every fresh open there is
        // nothing to render into yet -- `drawable` is weak, and the previous
        // film's view is long gone. Starting VLC now would be a race between
        // SwiftUI creating the view and VLC opening its video output, and a
        // file that starts fast wins it: VLC finds no NSView, gives up on the
        // picture for good ("No drawable-nsobject nor vout_window_t found")
        // and plays the sound over a black screen. Software-decoded AV1 is
        // quick enough off the mark to lose the race every time, while a
        // hardware-decoded H.264 usually reaches its first frame late enough
        // to survive it. So wait for the view instead of betting on it.
        if drawable != nil {
            beginPlayback()
        } else {
            playsWhenDrawableArrives = true
        }

        // Some files never reach .playing and never report .error either --
        // they just sit there. Don't leave a spinner up forever.
        openWatchdog?.cancel()
        openWatchdog = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 15_000_000_000)
            guard !Task.isCancelled, let self, !self.hasPlayed,
                  self.current?.id == item.id else { return }
            self.isBuffering = false
            self.isPlaying = false
            self.failure = "Ce fichier n'a pas pu être lu. Essayez « Ouvrir dans VLC »."
        }
    }

    /// Retire the current VLC player and start from a new one.
    ///
    /// A stopped player does not let go of its video output: the next file
    /// played through it picks the same one back up ("saving a free vout",
    /// "reusing provided vout"), still bound to the view and the display it
    /// was made for. So an output broken once -- opened while the screen was
    /// renegotiating its HDMI link -- stayed broken for every later film, and
    /// only a restart of the app brought the picture back. A new player costs
    /// next to nothing and always builds its output from scratch.
    private func freshEngine() {
        let old = player
        old.delegate = nil
        old.stop()
        old.drawable = nil
        player = VLCMediaPlayer()
        player.delegate = self
    }

    /// The view VLC renders into, handed over by the SwiftUI layer.
    weak var drawable: VLCVideoView? {
        didSet {
            guard drawable != nil else { return }
            player.drawable = drawable
            if playsWhenDrawableArrives { beginPlayback() }
        }
    }

    /// Set when `start` had no view to render into; the surface hands
    /// playback its cue as soon as SwiftUI has built it.
    private var playsWhenDrawableArrives = false

    /// Hand the file to VLC, with the render target already in place.
    private func beginPlayback() {
        playsWhenDrawableArrives = false
        player.drawable = drawable
        player.play()
        applyVolume()
    }

    // MARK: - Transport

    func togglePlay() {
        // Read the engine, not our cached flag, so a stale value cannot make
        // this a one-way switch.
        if player.isPlaying {
            player.pause()
            isPlaying = false
            pausedByUser = true
        } else if rebuildOnResume {
            pausedByUser = false
            rebuildVideoOutput()
        } else {
            player.play()
            isPlaying = true
            pausedByUser = false
        }
    }

    func seek(toFraction fraction: Double) {
        guard duration > 0 else { return }
        player.position = Float(max(0, min(1, fraction)))
        time = duration * fraction
    }

    func skip(_ seconds: Int) {
        if seconds >= 0 { player.jumpForward(Int32(seconds)) }
        else { player.jumpBackward(Int32(-seconds)) }
    }

    /// Two-finger trackpad gesture: raise or lower the volume.
    func nudgeVolume(by delta: Int) {
        volume = max(0, min(150, volume + delta))
        showVolumeHUD()
    }

    private func applyVolume() {
        player.audio?.volume = Int32(volume)
    }

    private func showVolumeHUD() {
        volumeHUD = volume
        hudTask?.cancel()
        hudTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_200_000_000)
            guard !Task.isCancelled else { return }
            self?.volumeHUD = nil
        }
    }

    func playNext() {
        guard !queue.isEmpty else { return }
        let next = queue.removeFirst()
        start(next, fromStart: true)
        // The controls are usually hidden when an episode rolls over, so say
        // out loud what just started.
        let label = [next.playerSubtitle, next.playerTitle]
            .filter { !$0.isEmpty }.joined(separator: " · ")
        showInfoHUD(label.isEmpty ? "Épisode suivant" : "À suivre · " + label)
    }

    /// Leave the player and go back to the library.
    func close() {
        openWatchdog?.cancel()
        stateTicker?.cancel()
        awaitingVolume?.cancel()
        awaitingVolume = nil
        displaySettle?.cancel()
        displaySettle = nil
        rebuildOnResume = false
        hudTask?.cancel()
        infoTask?.cancel()
        persistProgress(force: true)
        playsWhenDrawableArrives = false
        lastDeliberateStop = .now
        player.stop()
        player.media = nil
        current = nil
        lastEngineTime = -1
        lastProgressAt = .distantPast
        recoveryAttempts = 0
        lastRecoveryAt = .distantPast
        isPlaying = false
        allowSleep()
        isBuffering = false
        time = 0
        duration = 0
        queue = []
        audioTracks = []
        subtitleTracks = []
        currentAudioTrack = -1
        currentSubtitleTrack = -1
        volumeHUD = nil
        infoHUD = nil
    }

    // MARK: - Veille

    /// Held while a film is actually playing, so the display never dims and
    /// the screen never locks mid-episode. Released on pause and on exit --
    /// a paused player should not keep the Mac awake all night.
    private var sleepAssertion: IOPMAssertionID = 0

    private func updateSleepAssertion() {
        if current != nil && isPlaying { preventSleep() } else { allowSleep() }
    }

    private func preventSleep() {
        guard sleepAssertion == 0 else { return }
        var id: IOPMAssertionID = 0
        // "NoDisplaySleep" is what VLC itself holds: it stops the idle timers
        // that dim the screen, start the screen saver and lock the Mac.
        let result = IOPMAssertionCreateWithName(
            kIOPMAssertionTypeNoDisplaySleep as CFString,
            IOPMAssertionLevel(kIOPMAssertionLevelOn),
            "Biblifilm - lecture en cours" as CFString,
            &id)
        if result == kIOReturnSuccess { sleepAssertion = id }
    }

    private func allowSleep() {
        guard sleepAssertion != 0 else { return }
        IOPMAssertionRelease(sleepAssertion)
        sleepAssertion = 0
    }

    // MARK: - Tracks

    /// Poll VLC for the track lists. They appear a moment after playback
    /// starts, and a second subtitle file can show up later still, so this
    /// runs on the state ticker rather than once at open time.
    private func refreshTracks() {
        let audio = tracks(player.audioTrackIndexes, player.audioTrackNames)
        let subs = tracks(player.videoSubTitlesIndexes, player.videoSubTitlesNames)
        if audio != audioTracks { audioTracks = audio }
        if subs != subtitleTracks { subtitleTracks = subs }

        let live = Int(player.currentAudioTrackIndex)
        if live != currentAudioTrack { currentAudioTrack = live }
        let liveSub = Int(player.currentVideoSubTitleIndex)
        if liveSub != currentSubtitleTrack { currentSubtitleTrack = liveSub }

        applyPendingChoices()
    }

    /// VLC hands back parallel arrays of ids and labels, with a fake
    /// "Disable" entry at id -1 that we present ourselves.
    private func tracks(_ indexes: [Any]?, _ names: [Any]?) -> [MediaTrack] {
        guard let indexes, let names, indexes.count == names.count else { return [] }
        return zip(indexes, names).compactMap { index, name in
            guard let id = (index as? NSNumber)?.intValue, id >= 0,
                  let label = name as? String else { return nil }
            return MediaTrack(id: id, name: label)
        }
    }

    /// Re-apply the choice carried over from the previous episode, once the
    /// matching track exists. Kept up until playback is properly under way,
    /// because VLC may enable its own default track a moment after the list
    /// appears; a manual pick clears it, so this never fights the user.
    private func applyPendingChoices() {
        // Timed from when the tracks appear, not by playback position: a
        // film resumed at 45 minutes is past any position threshold on its
        // very first tick, long before VLC has listed a single track.
        guard !audioTracks.isEmpty || !subtitleTracks.isEmpty else { return }
        let seen = tracksSeenAt ?? .now
        tracksSeenAt = seen
        guard !(isPlaying && Date().timeIntervalSince(seen) > Self.trackSettleTime)
        else { return }

        if case .named(let name) = pendingAudio,
           let match = Self.bestMatch(for: name, in: audioTracks),
           match.id != currentAudioTrack {
            apply(audio: match.id)
        }

        switch pendingSubtitle {
        case .off where currentSubtitleTrack >= 0:
            apply(subtitle: -1)
        case .named(let name):
            if let match = Self.bestMatch(for: name, in: subtitleTracks),
               match.id != currentSubtitleTrack {
                apply(subtitle: match.id)
            }
        default:
            break
        }
    }

    private func apply(audio id: Int) {
        player.currentAudioTrackIndex = Int32(id)
        currentAudioTrack = id
    }

    private func apply(subtitle id: Int) {
        player.currentVideoSubTitleIndex = Int32(id)
        currentSubtitleTrack = id
    }

    /// Pick an audio track; the choice sticks for the rest of the queue.
    func selectAudioTrack(_ id: Int) {
        apply(audio: id)
        let name = audioTracks.first { $0.id == id }?.name
        stickyAudio = name.map { .named($0) } ?? .automatic
        pendingAudio = .automatic
        if let name, let current { Self.saveAudio(name, for: current) }
        showInfoHUD("Audio · " + (name.map(Self.displayName) ?? "Piste \(id)"))
    }

    /// Pick a subtitle track, or -1 for none; the choice sticks for the queue.
    func selectSubtitleTrack(_ id: Int) {
        apply(subtitle: id)
        if id < 0 {
            stickySubtitle = .off
            showInfoHUD("Sous-titres · Aucun")
        } else {
            let name = subtitleTracks.first { $0.id == id }?.name
            stickySubtitle = name.map { .named($0) } ?? .automatic
            showInfoHUD("Sous-titres · " + (name.map(Self.displayName) ?? "Piste \(id)"))
        }
        pendingSubtitle = .automatic
        if let current { Self.saveSubtitle(stickySubtitle, for: current) }
    }

    /// The track to re-select for a remembered label. Another file of the
    /// same show rarely spells it the same way ("Français VFF AC3" here,
    /// "Track 1 - [French]" there), so fall back to the same language --
    /// the same dub (VFF, VFQ) first if the file says which is which, and
    /// for subtitles forced or full as before -- forced French subtitles on
    /// an English track are not the same thing as full French subtitles.
    private static func bestMatch(for label: String, in tracks: [MediaTrack]) -> MediaTrack? {
        if let exact = tracks.first(where: { $0.name == label }) { return exact }
        guard let wanted = TrackLanguage(label: label) else { return nil }
        let candidates = tracks.compactMap { track -> (MediaTrack, Int)? in
            guard let language = TrackLanguage(label: track.name),
                  language.code == wanted.code else { return nil }
            let score = (language.forced == wanted.forced ? 2 : 0)
                      + (wanted.variant != nil && language.variant == wanted.variant ? 1 : 0)
            return (track, score)
        }
        // max(by:) keeps the last of equals; walk in order to keep the first.
        return candidates.reduce(nil as (MediaTrack, Int)?) { best, next in
            guard let best else { return next }
            return next.1 > best.1 ? next : best
        }?.0
    }

    // MARK: Remembered audio

    /// The audio track last picked for each film or series, kept across
    /// launches. Keyed by the parsed show, so every episode of a series
    /// shares one choice and it holds before any metadata has loaded.
    private static let audioDefaultsKey = "audioTrackByTitle"

    private static func savedAudio(for item: MediaItem) -> String? {
        let saved = UserDefaults.standard.dictionary(forKey: audioDefaultsKey)
        return saved?[item.parsed.showKey] as? String
    }

    private static func saveAudio(_ label: String, for item: MediaItem) {
        var saved = UserDefaults.standard.dictionary(forKey: audioDefaultsKey) ?? [:]
        saved[item.parsed.showKey] = label
        UserDefaults.standard.set(saved, forKey: audioDefaultsKey)
    }

    /// The subtitle choice for each film or series, stored the same way.
    /// "Aucun" is remembered too, so a series watched without subtitles
    /// does not get VLC's default track back on the next launch.
    private static let subtitleDefaultsKey = "subtitleTrackByTitle"
    private static let subtitlesOff = "\u{0}off"

    private static func savedSubtitle(for item: MediaItem) -> TrackChoice? {
        let saved = UserDefaults.standard.dictionary(forKey: subtitleDefaultsKey)
        guard let label = saved?[item.parsed.showKey] as? String else { return nil }
        return label == subtitlesOff ? .off : .named(label)
    }

    private static func saveSubtitle(_ choice: TrackChoice, for item: MediaItem) {
        var saved = UserDefaults.standard.dictionary(forKey: subtitleDefaultsKey) ?? [:]
        switch choice {
        case .off:               saved[item.parsed.showKey] = subtitlesOff
        case .named(let label):  saved[item.parsed.showKey] = label
        case .automatic:         return
        }
        UserDefaults.standard.set(saved, forKey: subtitleDefaultsKey)
    }

    /// Keyboard shortcut "b", as in VLC.
    func cycleAudioTrack() {
        guard audioTracks.count > 1 else { return }
        let index = audioTracks.firstIndex { $0.id == currentAudioTrack } ?? -1
        selectAudioTrack(audioTracks[(index + 1) % audioTracks.count].id)
    }

    /// Keyboard shortcut "v", as in VLC. Cycles through "none" as well.
    func cycleSubtitleTrack() {
        guard !subtitleTracks.isEmpty else { return }
        let ids = subtitleTracks.map(\.id) + [-1]
        let index = ids.firstIndex(of: currentSubtitleTrack) ?? ids.count - 1
        selectSubtitleTrack(ids[(index + 1) % ids.count])
    }

    /// VLC labels tracks "Français VFi AC3 5.1 @ 448Kbps - [French]", or just
    /// "Track 2 - [French]" when the file names nothing. Keep the descriptive
    /// half: the label itself, or the language when the label is only a number.
    static func displayName(_ raw: String) -> String {
        guard let open = raw.lastIndex(of: "["), let close = raw.lastIndex(of: "]"),
              open < close else { return raw }
        let language = raw[raw.index(after: open)..<close]
            .trimmingCharacters(in: .whitespaces)
        var head = String(raw[raw.startIndex..<open])
            .trimmingCharacters(in: .whitespaces)
        if head.hasSuffix("-") { head.removeLast() }
        head = head.trimmingCharacters(in: .whitespaces)

        let generic = head.isEmpty
            || head.range(of: "^(Track|Piste)\\s*\\d+$",
                          options: [.regularExpression, .caseInsensitive]) != nil
        if generic { return language.isEmpty ? raw : language }
        return head
    }

    private func showInfoHUD(_ text: String) {
        infoHUD = text
        infoTask?.cancel()
        infoTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_600_000_000)
            guard !Task.isCancelled else { return }
            self?.infoHUD = nil
        }
    }

    // MARK: - Progress

    /// Write the exact position now. Called when the app is quitting, so a
    /// ⌘Q mid-episode does not lose up to two seconds of viewing -- enough,
    /// at the start of a file, to lose the viewing altogether.
    func persistNow() { persistProgress(force: true) }

    private func persistProgress(force: Bool = false) {
        guard let current, duration > 0 else { return }
        // Read the engine rather than the cached `time`: time notifications
        // can go quiet, and a position that stops advancing is a viewing lost.
        let engine = Double(player.time.intValue) / 1000.0
        let position = engine > 0 ? engine : time
        guard position > 0 else { return }
        if !force {
            if Date().timeIntervalSince(lastPersist) < 2 { return }
            // Nothing moved (paused, or stalled): don't rewrite the history
            // file every two seconds for the same position.
            if abs(position - lastPersistedPosition) < 0.5 { return }
        }
        lastPersist = .now
        lastPersistedPosition = position
        WatchStore.shared.update(path: current.id, position: position, duration: duration)
    }

    fileprivate func handleTimeChanged() {
        time = Double(player.time.intValue) / 1000.0

        // State notifications can be missed (the transition to .playing may
        // land before the delegate is attached), which would leave the spinner
        // up and the pause button showing "play". Time ticks are reliable, so
        // trust them for liveness.
        isPlaying = player.isPlaying
        if isPlaying { isBuffering = false }
        if duration > 0 && time > 3 {
            hasPlayed = true
            openWatchdog?.cancel()
        }
        if duration <= 0 {
            let length = player.media?.length.intValue ?? 0
            if length > 0 { duration = Double(length) / 1000.0 }
        }
        // Seeking is only possible once VLC knows the stream; do it on the
        // first tick rather than immediately after play().
        if let resume = pendingResume, duration > 0, player.isSeekable {
            pendingResume = nil
            player.time = VLCTime(int: Int32(resume * 1000))
        }
        persistProgress()
    }

    fileprivate func handleStateChanged() {
        switch player.state {
        case .playing:
            isPlaying = true
            isBuffering = false
            refreshTracks()
        case .paused:
            isPlaying = false
        case .buffering, .opening:
            isBuffering = true
        case .ended:
            isPlaying = false
            finishCurrent()
        case .stopped:
            // libvlc tearing the input down by itself, part way through a
            // film. This used to fall through to `default` and be ignored
            // outright, which is precisely how a frozen picture could sit
            // there for good. Our own stops -- closing the player, reloading
            // after a freeze -- come through here too, hence the guards.
            isPlaying = false
            if current != nil, hasPlayed, !pausedByUser, failure == nil,
               awaitingVolume == nil,
               Date().timeIntervalSince(lastDeliberateStop) > 2 {
                restartPlayback(reason: "VLC a arrêté la lecture")
            }
        case .error:
            isPlaying = false
            isBuffering = false
            // An error on a film already running is a hiccup worth reloading
            // through; one on a file that never started is a file we cannot
            // play, and saying so is more use than trying again.
            if current != nil, hasPlayed {
                restartPlayback(reason: "erreur VLC en cours de lecture")
            } else {
                failure = "Lecture impossible. Essayez « Ouvrir dans VLC »."
            }
        default:
            break
        }
    }

    /// How close to the duration counts as the end. VLCKit stops a few
    /// hundred milliseconds short; two seconds is comfortably past that and
    /// still inside the credits.
    private static let endOfFileSlack: Double = 2.0

    /// VLCKit 3.7 does not report `.ended` for our files: on the last frame
    /// the player simply parks on `paused`, a fraction of a second short of
    /// the duration. Waiting for that state meant an episode never finished,
    /// so the next one never started. The clock is the reliable signal.
    private func checkForEndOfFile() {
        guard current != nil, hasPlayed, !pausedByUser,
              duration > 0, !player.isPlaying,
              time >= duration - Self.endOfFileSlack else { return }
        finishCurrent()
    }

    // MARK: - Gels

    /// The freeze this guards against: part way through a film, libvlc tears
    /// its input down and closes the file. Nothing is reported that the app
    /// used to act on -- not `.ended`, not `.error` -- so the picture simply
    /// stopped on its last frame and stayed there until the app was
    /// restarted. VLCKit says the same handful of things whether playback is
    /// alive or dead, so the clock is what we watch: when it stands still
    /// while nobody asked for a pause, playback is gone.
    private func checkForStall() {
        guard current != nil, hasPlayed, !pausedByUser, failure == nil,
              awaitingVolume == nil else { return }
        // The last seconds of a file legitimately stop the clock. That is
        // checkForEndOfFile's business, not this one.
        if duration > 0 && time >= duration - Self.endOfFileSlack { return }

        let engine = player.time.intValue
        if engine != lastEngineTime {
            lastEngineTime = engine
            lastProgressAt = .now
            return
        }
        guard Date().timeIntervalSince(lastProgressAt) >= Self.stallTimeout else { return }
        restartPlayback(reason: "horloge figée à \(Int(time))s")
    }

    /// Reload the file where it stopped and carry on. This is what restarting
    /// the app used to achieve, minus the restart: the queue, the volume and
    /// the chosen audio and subtitle tracks all come back with it.
    private func restartPlayback(reason: String) {
        guard let item = current, awaitingVolume == nil else { return }

        // Nothing to recover at the very end of a file -- roll on instead.
        if duration > 0 && time >= duration - Self.endOfFileSlack {
            finishCurrent()
            return
        }

        // Usually this is not VLC's doing at all: an external drive drops off
        // the USB bus, takes the file with it and comes back a few seconds
        // later. Waiting is the whole of the fix -- reloading against a path
        // that is not there would burn the retry budget in twenty seconds and
        // put an error up for a disk already on its way back.
        if !FileManager.default.fileExists(atPath: item.url.path) {
            waitForFile(item, resumingAt: max(0, time - 3), reason: reason)
            return
        }

        // A film that has run cleanly for a minute and then trips again gets
        // a fresh budget; three failures in quick succession is a file we
        // cannot read, not a hiccup.
        if Date().timeIntervalSince(lastRecoveryAt) > 60 { recoveryAttempts = 0 }

        VLCDiagnostics.shared.dump(
            reason: "\(item.url.lastPathComponent) — \(reason) — reprise "
                  + "\(recoveryAttempts + 1)/\(Self.maxRecoveryAttempts)")

        guard recoveryAttempts < Self.maxRecoveryAttempts else {
            isPlaying = false
            isBuffering = false
            failure = "La lecture s'est arrêtée et n'a pas pu repartir. "
                    + "Essayez « Ouvrir dans VLC »."
            return
        }
        recoveryAttempts += 1
        lastRecoveryAt = .now

        // Land a moment before the freeze rather than on it: VLC seeks to the
        // nearest keyframe, and coming in just ahead of whatever stopped it
        // is what lets the demuxer read through that spot.
        let resumeAt = max(0, time - 3)
        persistProgress(force: true)

        load(item, resumingAt: resumeAt)
        showInfoHUD("Reprise de la lecture…")
    }

    // MARK: - Écrans

    /// A display came, went or changed mode. Wait for things to settle, then
    /// have VLC build its video output again.
    fileprivate func displaysChanged() {
        guard current != nil else { return }
        displaySettle?.cancel()
        displaySettle = Task { [weak self] in
            try? await Task.sleep(nanoseconds: Self.displaySettleDelay)
            guard !Task.isCancelled else { return }
            self?.rebuildVideoOutput()
        }
    }

    /// VLC sets its OpenGL output up once, against the display of the moment,
    /// and never revisits it. When that display goes away under it the sound
    /// carries on and the picture stays black for good. So the film is
    /// reloaded through a fresh player (`load` always starts one), which
    /// builds a new output in the same view, at the second it had reached.
    private func rebuildVideoOutput() {
        displaySettle = nil
        guard let item = current, failure == nil, awaitingVolume == nil else { return }

        if pausedByUser {
            rebuildOnResume = true
            return
        }

        let resumeAt = pendingResume ?? (time > 0 ? time : nil)
        persistProgress(force: true)

        load(item, resumingAt: resumeAt)
        showInfoHUD("Écran reconfiguré — image rétablie")
    }

    /// Sit out a drive that has gone away, and pick the film back up the
    /// moment it returns -- at the second it left, not at the start.
    private func waitForFile(_ item: MediaItem, resumingAt resume: Double,
                             reason: String) {
        lastDeliberateStop = .now
        player.stop()
        isPlaying = false
        isBuffering = true
        persistProgress(force: true)
        VLCDiagnostics.shared.dump(
            reason: "\(item.url.lastPathComponent) — \(reason) — fichier "
                  + "introuvable, attente du disque")
        showInfoHUD("Disque déconnecté — reprise dès son retour…")

        awaitingVolume = Task { [weak self] in
            let deadline = Date().addingTimeInterval(Self.volumeWaitTimeout)
            while Date() < deadline {
                try? await Task.sleep(nanoseconds: 1_000_000_000)
                guard !Task.isCancelled, let self, self.current?.id == item.id
                else { return }
                guard FileManager.default.fileExists(atPath: item.url.path)
                else { continue }
                self.awaitingVolume = nil
                // The drive coming back is a clean slate: what follows is a
                // fresh playback, not another go at a failing one.
                self.recoveryAttempts = 0
                self.load(item, resumingAt: resume)
                self.showInfoHUD("Reprise de la lecture…")
                return
            }
            guard let self, self.current?.id == item.id else { return }
            self.awaitingVolume = nil
            self.isBuffering = false
            self.failure = "Le disque contenant ce film s'est déconnecté."
        }
    }

    /// At the end of a file: mark it seen and roll on, Netflix-style.
    ///
    /// A file that fails to open also reports "ended", instantly and with no
    /// duration. Without this guard the queue burns through a whole season,
    /// marking every episode as watched without playing a frame.
    private func finishCurrent() {
        guard hasPlayed else {
            isPlaying = false
            isBuffering = false
            failure = "Ce fichier n'a pas pu être lu. Essayez « Ouvrir dans VLC »."
            return
        }

        if let current, duration > 0, time >= duration * WatchRecord.completionThreshold {
            WatchStore.shared.setWatched(true, for: current)
        }
        if queue.isEmpty { close() } else { playNext() }
    }
}

extension PlayerModel: VLCMediaPlayerDelegate {
    nonisolated func mediaPlayerTimeChanged(_ aNotification: Notification) {
        Task { @MainActor in self.handleTimeChanged() }
    }

    nonisolated func mediaPlayerStateChanged(_ aNotification: Notification) {
        Task { @MainActor in self.handleStateChanged() }
    }
}

/// Formats seconds as 1:02:03 / 4:05.
func formatTimecode(_ seconds: Double) -> String {
    guard seconds.isFinite, seconds >= 0 else { return "0:00" }
    let total = Int(seconds)
    let h = total / 3600, m = (total % 3600) / 60, s = total % 60
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, s)
                 : String(format: "%d:%02d", m, s)
}
