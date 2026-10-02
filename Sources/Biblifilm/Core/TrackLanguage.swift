import Foundation

/// The language a track label speaks of, whatever way the release spells it.
/// "Français VFF AC3 5.1 - [French]", "Track 2 - [fre]" and "VF - [FR]" are
/// all French; the variant (VFF, VFQ, VFI) tells a France dub from a Quebec one.
struct TrackLanguage: Equatable {
    let code: String
    let variant: String?
    /// Subtitles that only cover foreign-language lines ("Français (forcés)").
    let forced: Bool

    /// Every spelling seen in the wild, lowercased, mapped to one code.
    private static let aliases: [String: String] = {
        let table: [String: [String]] = [
            "fr": ["fr", "fre", "fra", "french", "français", "francais", "française",
                   "francaise", "vf", "vff", "vfq", "vfi", "vf2", "vfb", "truefrench"],
            "en": ["en", "eng", "english", "anglais", "anglaise"],
            "es": ["es", "spa", "esp", "spanish", "español", "espanol", "espagnol",
                   "castellano", "latino"],
            "de": ["de", "ger", "deu", "german", "deutsch", "allemand"],
            "it": ["it", "ita", "italian", "italiano", "italien"],
            "pt": ["pt", "por", "portuguese", "português", "portugues", "portugais"],
            "ja": ["ja", "jpn", "jap", "japanese", "japonais", "日本語"],
            "ko": ["ko", "kor", "korean", "coréen", "coreen", "한국어"],
            "zh": ["zh", "chi", "zho", "chinese", "chinois", "mandarin", "cantonese"],
            "ru": ["ru", "rus", "russian", "russe", "русский"],
            "nl": ["nl", "dut", "nld", "dutch", "néerlandais", "neerlandais", "nederlands"],
            "ar": ["ar", "ara", "arabic", "arabe"],
            "pl": ["pl", "pol", "polish", "polonais", "polski"],
            "sv": ["sv", "swe", "swedish", "suédois", "suedois", "svenska"],
            "da": ["da", "dan", "danish", "danois", "dansk"],
            "no": ["no", "nor", "nob", "norwegian", "norvégien", "norvegien", "norsk"],
            "fi": ["fi", "fin", "finnish", "finnois", "suomi"],
            "tr": ["tr", "tur", "turkish", "turc", "türkçe"],
            "hi": ["hi", "hin", "hindi"],
        ]
        var map: [String: String] = [:]
        for (code, words) in table { for word in words { map[word] = code } }
        return map
    }()

    /// Spellings that say which dub it is, not just which language.
    private static let variants: Set<String> = ["vff", "vfq", "vfi", "vf2", "vfb", "truefrench"]

    /// Two-letter codes like "no" or "it" are ordinary words too; only trust
    /// them inside VLC's "[…]" suffix, where they can only mean a language.
    private static let ambiguous: Set<String> = ["no", "it", "de", "es", "en", "da", "fi", "hi"]

    private static let forcedWords: Set<String> = ["forced", "forcé", "forcés", "forcée",
                                                   "forcées", "force", "forces", "forcee", "forcees"]

    init?(label: String) {
        var bracket: String?
        if let open = label.lastIndex(of: "["), let close = label.lastIndex(of: "]"),
           open < close {
            bracket = String(label[label.index(after: open)..<close])
        }
        let words = Self.words(in: label)
        let variant = words.first { Self.variants.contains($0) }
        let forced = words.contains { Self.forcedWords.contains($0) }

        // VLC's own language tag is the most reliable, then the free text.
        if let bracket,
           let code = Self.words(in: bracket).lazy.compactMap({ Self.aliases[$0] }).first {
            self.init(code: code, variant: variant, forced: forced)
            return
        }
        if let code = words.lazy
            .filter({ !Self.ambiguous.contains($0) })
            .compactMap({ Self.aliases[$0] }).first {
            self.init(code: code, variant: variant, forced: forced)
            return
        }
        return nil
    }

    private init(code: String, variant: String?, forced: Bool) {
        self.code = code
        self.variant = variant
        self.forced = forced
    }

    private static func words(in text: String) -> [String] {
        text.lowercased()
            .components(separatedBy: CharacterSet.alphanumerics.inverted)
            .filter { !$0.isEmpty }
    }
}
