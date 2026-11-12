//! Transforme un nom de fichier de release en titre propre, avec saison et
//! épisode. Traduction directe de FilenameParser.swift.
//!
//!   [DragonMax] Nicky Larson - 001.mkv        -> épisode, numéro absolu 1
//!   Breaking.Bad.S01E02.1080p.WEB-DL.mkv      -> épisode S1E2
//!   The Wire 3x04 - Amsterdam.avi             -> épisode S3E4
//!   Inception.2010.1080p.BluRay.x264.mkv      -> film, 2010

use fancy_regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind { Movie, Episode, Audio, Other }

impl Kind {
    pub fn name(self) -> &'static str { match self { Kind::Movie => "Film", Kind::Episode => "Série", Kind::Audio => "Audio", Kind::Other => "Autre" } }
    pub fn icon(self) -> &'static str { match self { Kind::Movie => "film", Kind::Episode => "tv", Kind::Audio => "music", Kind::Other => "question" } }
    pub fn rank(self) -> u8 { match self { Kind::Audio => 0, Kind::Episode => 1, Kind::Movie => 2, Kind::Other => 3 } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Special { Film, Oav, Special, Extra }

impl Special {
    pub fn bucket(self) -> &'static str { match self { Special::Extra => "Bonus", _ => "Films et hors-séries" } }
    pub fn row(self) -> &'static str { match self { Special::Film => "Film", Special::Oav => "OAV", Special::Special => "Spécial", Special::Extra => "Bonus" } }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parsed {
    pub title: String,
    pub kind: Kind,
    pub year: Option<i32>,
    pub season: Option<i32>,
    pub episode: Option<i32>,
    pub absolute_episode: Option<i32>,
    pub release_group: Option<String>,
    pub special: Option<Special>,
    pub special_title: Option<String>,
    pub special_number: Option<i32>,
}

impl Parsed {
    /// Clé commune à tous les épisodes d'une série.
    pub fn show_key(&self) -> String {
        let mut k = self.title.to_lowercase();
        if let Some(y) = self.year { k += &format!("|{y}"); }
        k + "|" + match self.kind { Kind::Movie => "movie", Kind::Episode => "episode", Kind::Audio => "audio", Kind::Other => "other" }
    }
}

pub const VIDEO: &[&str] = &["mkv", "mp4", "m4v", "avi", "mov", "wmv", "flv", "webm", "mpg", "mpeg", "mpe", "m2v", "m2ts", "mts", "ts",
    "vob", "ogv", "ogm", "3gp", "3g2", "divx", "xvid", "rmvb", "rm", "asf", "f4v", "mxf", "nsv", "iso", "dv", "amv", "mp2", "mpv", "qt", "yuv", "m4p"];
pub const AUDIO: &[&str] = &["mp3", "flac", "m4a", "aac", "wav", "aiff", "aif", "ogg", "oga", "opus", "wma", "alac", "ape", "wv", "mka",
    "dsf", "dff", "mid", "midi", "it", "mod", "s3m", "xm", "spx", "tta", "voc", "au"];

const JUNK: &[&str] = &["480p", "576p", "720p", "1080p", "1080i", "2160p", "4k", "uhd", "hd", "sd", "bluray", "blu-ray", "brrip", "bdrip",
    "bdremux", "remux", "webrip", "web-dl", "webdl", "web", "hdtv", "pdtv", "dvdrip", "dvdscr", "dvd", "hdrip", "camrip", "cam", "ts", "tc",
    "r5", "vhsrip", "satrip", "dsr", "hdcam", "x264", "x265", "h264", "h265", "h", "avc", "hevc", "xvid", "divx", "10bit", "8bit", "10bits",
    "hi10p", "hdr", "hdr10", "dovi", "dv", "sdr", "aac", "aac2", "ac3", "eac3", "dts", "dtshd", "truehd", "atmos", "flac", "mp3", "opus",
    "ddp", "ddp5", "dd5", "dd", "2ch", "6ch", "multi", "multi3", "vostfr", "vost", "vf", "vff", "vfq", "vfi", "vo", "french", "truefrench",
    "english", "eng", "fre", "fr", "jap", "jpn", "subfrench", "subbed", "dubbed", "dual", "audio", "repack", "proper", "rerip", "internal",
    "limited", "unrated", "extended", "remastered", "directors", "cut", "theatrical", "complete", "integrale", "final", "uncensored", "bd",
    "raw", "batch", "nf", "amzn", "dsnp", "atvp", "hmax", "hulu", "itunes", "criterion", "version", "longue", "vlong", "subforced", "subs",
    "sub", "hdlight", "light", "uncut", "3d", "sbs", "readnfo", "hardcoded"];

fn re(s: &str) -> Regex { Regex::new(s).unwrap() }

static EPISODE: LazyLock<Vec<Regex>> = LazyLock::new(|| vec![
    re(r"[sS](\d{1,2})[\s._-]*[eE](\d{1,3})"),
    re(r"(?<![0-9a-zA-Z])(\d{1,2})[xX](\d{1,3})(?![0-9])"),
    re(r"[sS]aison[\s._-]*(\d{1,2})[\s._-]*[eEéÉ](?:p(?:isode)?)?[\s._-]*(\d{1,3})"),
    re(r"[sS]eason[\s._-]*(\d{1,2})[\s._-]*[eE](?:p(?:isode)?)?[\s._-]*(\d{1,3})"),
]);

// Un hors-série n'est reconnu qu'après un tiret séparateur : sinon « The Movie
// Critic » ou « Film Noir Collection » passeraient pour des hors-séries.
static SPECIALS: LazyLock<Vec<(Regex, Special)>> = LazyLock::new(|| {
    let sep = r"[\s._]+[-–][\s._]*";
    let strong = [(r"t[ée]l[ée]films?", Special::Film), (r"oavs?|ovas?", Special::Oav),
        (r"ncop|nced|openings?|endings?|g[ée]n[ée]riques?", Special::Extra), (r"bonus|making[\s._-]?of", Special::Extra)];
    let numbered = [(r"films?", Special::Film), (r"movies?", Special::Film), (r"sp[ée]ciaux|sp[ée]ciales?|specials?", Special::Special)];
    let mut v: Vec<(Regex, Special)> = strong.iter().map(|(w, k)| (re(&format!("(?i){sep}(?:{w})(?![a-z])")), *k)).collect();
    v.extend(numbered.iter().map(|(w, k)| (re(&format!(r"(?i){sep}(?:{w})(?![a-z])(?=[\s._-]*\d)")), *k)));
    v
});

static ABSOLUTE: LazyLock<Vec<Regex>> = LazyLock::new(|| vec![
    re(r"[\s._-]-[\s._]*(\d{1,4})(?:v\d)?(?=[\s._\[(]|$)"),
    re(r"(?<![a-zA-Z0-9])[eEéÉ](?:[pP](?:isode|is)?)?[\s._-]*(\d{1,4})(?![0-9])"),
    re(r"(?<![a-zA-Z0-9])#[\s._]*(\d{1,4})(?![0-9])"),
]);

struct M { start: usize, end: usize, groups: Vec<String> }

fn find(s: &str, r: &Regex) -> Option<M> {
    let c = r.captures(s).ok()??;
    let m = c.get(0)?;
    Some(M { start: m.start(), end: m.end(), groups: (1..c.len()).map(|i| c.get(i).map(|g| g.as_str().to_string()).unwrap_or_default()).collect() })
}

fn replace_all(s: &str, pattern: &str, with: &str) -> String { re(pattern).replace_all(s, with).to_string() }

fn trim_chars(s: &str, chars: &str) -> String { s.trim_matches(|c: char| chars.contains(c)).to_string() }

fn is_noise_only(title: &str) -> bool {
    replace_all(&title.to_lowercase(), r"\b(saison|season|episode|episodes|ep|epis|serie|series|s|e|vol|volume|part|partie|disc|disque|livre|book|cd|dvd)\b|\d+", " ").trim().is_empty()
}

/// Séparateurs normalisés, déchets de release retirés.
fn clean_title(input: &str) -> String {
    let mut s = replace_all(input, r"(?<![0-9])[0-9]\.[0-9](?![0-9])", " ");
    s = replace_all(&s, r"\([^)]*\)", " ");
    s = s.replace('_', " ");
    s = replace_all(&s, r"(?<=[a-zA-Z0-9])\.(?=[a-zA-Z0-9])", " ");
    s = s.replace('.', " ");
    s = replace_all(&s, r"[()\[\]{}]", " ");
    s = replace_all(&s, r"\s-\s*[A-Za-z0-9]{2,15}\s*$", " ");
    let kept: Vec<&str> = s.split([' ', '-']).filter(|t| {
        let low = t.to_lowercase();
        let low = low.trim_matches(|c: char| c.is_ascii_punctuation());
        !low.is_empty() && !JUNK.contains(&low)
    }).collect();
    let out = replace_all(&kept.join(" "), r"\s{2,}", " ");
    trim_chars(&out, " -–—_.,:;")
}

/// `folder_hint` : nom du dossier parent, titre de secours quand le nom du
/// fichier n'en porte aucun (`Kaamelott/Saison 2 Episode 5.mkv`).
pub fn parse(file: &Path, folder_hint: Option<&str>) -> Parsed {
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let raw: String = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default().nfc().collect();
    let mut work = raw.clone();
    let release_group = find(&work, &re(r"^\[([^\]]{1,30})\]")).map(|m| m.groups[0].clone());
    work = replace_all(&work, r"\[[^\]]*\]", " ");
    work = replace_all(&work, r"\{[^}]*\}", " ");

    // Année : entre parenthèses, elle fait foi (« Blade Runner 2049 (2017) »).
    let year_m = find(&work, &re(r"\((19\d{2}|20\d{2})\)")).or_else(|| find(&work, &re(r"(?<![0-9])(19\d{2}|20\d{2})(?![0-9])")));
    let year = year_m.as_ref().and_then(|m| m.groups[0].parse().ok());

    // Films, OAV et bonus : hors de la numérotation des épisodes.
    let (mut special, mut special_title, mut special_number, mut special_range) = (None, None, None, None);
    for (r, kind) in SPECIALS.iter() {
        let Some(m) = find(&work, r) else { continue };
        special = Some(*kind);
        let tail = work[m.end..].to_string();
        if let Some(n) = find(&tail, &re(r"^[\s._-]*(\d{1,3})(?![0-9])")) {
            special_number = n.groups[0].parse().ok();
            special_title = Some(clean_title(&tail[n.end..]));
        } else if *kind == Special::Extra {
            special_title = Some(clean_title(&work[m.start..]));
        } else {
            special_title = Some(clean_title(&tail));
        }
        if special_title.as_deref() == Some("") { special_title = None; }
        special_range = Some(m);
        break;
    }

    let (mut season, mut episode, mut se_range) = (None, None, None);
    if special.is_none() {
        for r in EPISODE.iter() {
            if let Some(m) = find(&work, r) {
                season = m.groups[0].parse().ok();
                episode = m.groups[1].parse().ok();
                se_range = Some(m);
                break;
            }
        }
    }

    let (mut absolute, mut abs_range) = (None, None);
    if season.is_none() && special.is_none() {
        for r in ABSOLUTE.iter() {
            if let Some(m) = find(&work, r) {
                let n: i32 = m.groups[0].parse().unwrap_or(0);
                if !(1900..=2099).contains(&n) { absolute = Some(n); abs_range = Some(m); break; }
            }
        }
    }

    let cut = special_range.as_ref().or(se_range.as_ref()).or(abs_range.as_ref()).or(year_m.as_ref()).map(|m| m.start);
    let mut title_part = cut.map(|c| work[..c].to_string()).unwrap_or_else(|| work.clone());
    // « Monster.2004.S01E61 » : une année finale appartient à la série.
    if season.is_some() || absolute.is_some() {
        title_part = replace_all(&title_part, r"[\s._-]*(?<![0-9])(19\d{2}|20\d{2})(?![0-9])[\s._-]*$", " ");
    }
    let mut title = clean_title(&title_part);
    if title.is_empty() || is_noise_only(&title) {
        if let Some(h) = folder_hint {
            let hinted = clean_title(&h.nfc().collect::<String>());
            if !hinted.is_empty() && !is_noise_only(&hinted) { title = hinted; }
        }
    }
    if title.is_empty() { title = clean_title(&work); }
    if title.is_empty() { title = raw.clone(); }

    let kind = if AUDIO.contains(&ext.as_str()) { Kind::Audio }
        else if special.is_some() || season.is_some() || absolute.is_some() { Kind::Episode }
        else if VIDEO.contains(&ext.as_str()) { Kind::Movie } else { Kind::Other };

    Parsed { title, kind, year, season, episode, absolute_episode: absolute, release_group, special, special_title, special_number }
}

/// Un titre tapé à la main : seule une année finale est retirée (et rendue à part).
pub fn normalize_user_title(input: &str) -> (String, Option<i32>) {
    let composed: String = input.nfc().collect::<String>().trim().to_string();
    let trailing = r"[\s(\[]*(?<![0-9])(19\d{2}|20\d{2})(?![0-9])[\s)\]]*$";
    let year = find(&composed, &re(trailing)).and_then(|m| m.groups[0].parse().ok());
    let s = replace_all(&replace_all(&composed, trailing, " "), r"\s{2,}", " ");
    let s = trim_chars(&s, " -–—_.,:;()[]");
    (if s.is_empty() { composed } else { s }, year)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(name: &str) -> Parsed { parse(Path::new(&format!("/media/{name}")), None) }

    #[test]
    fn episodes() {
        for (f, t, s, e) in [("Breaking.Bad.S01E02.1080p.WEB-DL.mkv", "Breaking Bad", 1, 2), ("The Wire 3x04 - Amsterdam.avi", "The Wire", 3, 4),
            ("Sakamoto Days S01E20 MULTi 1080p WEB AV1 E-AC-3 -Tsundere-Raws.mkv", "Sakamoto Days", 1, 20),
            ("Kaamelott Saison 2 Episode 5.mkv", "Kaamelott", 2, 5), ("Show.Name.s02.e10.mkv", "Show Name", 2, 10)] {
            let x = p(f);
            assert_eq!((x.kind, x.title.as_str(), x.season, x.episode), (Kind::Episode, t, Some(s), Some(e)), "{f}");
        }
    }

    #[test]
    fn movies() {
        for (f, t, y) in [("Inception.2010.1080p.BluRay.x264.mkv", "Inception", 2010),
            ("Le Fabuleux Destin d'Amélie Poulain (2001).mp4", "Le Fabuleux Destin d'Amélie Poulain", 2001),
            ("Blade Runner 2049 (2017).mkv", "Blade Runner 2049", 2017), ("Marionnette.2020.MULTI.1080p.WEB.H264-EXTREME.mkv", "Marionnette", 2020),
            ("Buddy.2026.VOSTFR.1080p.WEBRip.10.bits.EAC3.5.1.x265-GL0P.mkv", "Buddy", 2026),
            ("Il est de retour.2015.MULTi.1080p.BluRay.HEVC.DTS.HDMA.5.1-AKLHD.mkv", "Il est de retour", 2015)] {
            let x = p(f);
            assert_eq!((x.kind, x.title.as_str(), x.year), (Kind::Movie, t, Some(y)), "{f}");
        }
    }

    #[test]
    fn absolute_and_group() {
        let x = p("[DragonMax] Nicky Larson - 001.mkv");
        assert_eq!((x.kind, x.title.as_str(), x.absolute_episode, x.season, x.release_group.as_deref()), (Kind::Episode, "Nicky Larson", Some(1), None, Some("DragonMax")));
        let m = p("Monster.2004.S01E61.mkv");
        assert_eq!((m.title.as_str(), m.year, m.season, m.episode), ("Monster", Some(2004), Some(1), Some(61)));
        assert_eq!(p("Some Show - 2004.mkv").absolute_episode, None);
    }

    #[test]
    fn specials() {
        let o = p("Nicky Larson - OAV - 01 - Bay City Wars.mkv");
        assert_eq!((o.special, o.special_number, o.special_title.as_deref(), o.absolute_episode, o.title.as_str()), (Some(Special::Oav), Some(1), Some("Bay City Wars"), None, "Nicky Larson"));
        assert_eq!(p("The Movie Critic.mkv").special, None);
        assert_eq!(p("Film Noir Collection.mkv").special, None);
        let nfd: String = "Nicky Larson - Téléfilm - Goodbye.mkv".nfd().collect();
        assert_eq!(p(&nfd).special, Some(Special::Film));
    }

    #[test]
    fn folder_and_kinds() {
        let x = parse(Path::new("/media/Saison 2 Episode 5.mkv"), Some("Kaamelott"));
        assert_eq!((x.title.as_str(), x.season, x.episode), ("Kaamelott", Some(2), Some(5)));
        assert_eq!(p("Song.flac").kind, Kind::Audio);
        assert_eq!(p("Notes.txt").kind, Kind::Other);
        assert_eq!(p("Some Film.mkv").kind, Kind::Movie);
        assert_eq!(p("Breaking.Bad.S01E02.mkv").show_key(), p("Breaking Bad S03E10 720p.mkv").show_key());
    }

    #[test]
    fn user_title() {
        assert_eq!(normalize_user_title("Pusher II (2004)"), ("Pusher II".into(), Some(2004)));
        assert_eq!(normalize_user_title("The Cut"), ("The Cut".into(), None));
        assert_eq!(normalize_user_title("  Alien 1979 "), ("Alien".into(), Some(1979)));
    }
}
