use regex::Regex;

use crate::iptv::errors::IptvError;

/// Normalize a server URL: strip trailing slashes, validate scheme.
pub fn normalize_server_url(url: &str) -> Result<String, IptvError> {
    let trimmed = url.trim().trim_end_matches('/');

    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(IptvError::InvalidServer);
    }

    // Reject URLs with embedded credentials.
    if let Ok(parsed) = url::Url::parse(trimmed) {
        if parsed.username() != "" || parsed.password().is_some() {
            return Err(IptvError::InvalidServer);
        }
    } else {
        return Err(IptvError::InvalidServer);
    }

    Ok(trimmed.to_string())
}

/// Parse a category name into (country, clean_name).
///
/// Xtream providers use a dozen different conventions for the country
/// prefix — sometimes a 2-3 char code (`IN:`, `PK:`), sometimes a full
/// country name (`INDIA:`, `PAKISTAN:`), sometimes with emoji or pipes
/// (`IN 🇮🇳:`, `IN | HINDI`), sometimes no prefix at all. We try each
/// form in order of specificity so the most precise match wins.
///
/// Returns `(None, cleaned_name)` when no country can be identified.
#[allow(dead_code)]
pub fn parse_category_name(raw: &str) -> (Option<String>, String) {
    let cleaned = clean_category_suffix(raw);

    // 1. 2-3 char uppercase code at start, followed by `:` (the original
    //    pattern — matches `DK: SPORT`, `USA: NEWS`).
    let re_short = Regex::new(r"^([A-Z]{2,3}):\s*(.+)$").unwrap();
    if let Some(caps) = re_short.captures(&cleaned) {
        let code = caps[1].to_string();
        // Only accept real country codes — `XX:`, `ZZ:`, etc. are
        // not countries and should fall through.
        if country_code_to_name(&code).is_some() {
            return (Some(code), clean_category_suffix(&caps[2]));
        }
    }

    // 2. 2-3 char code (any case) at start, followed by `:`, `|`, or ` -`.
    //    Catches `IN | HINDI`, `IN - HINDI`, `in: news`.
    let re_loose = Regex::new(r"^([A-Za-z]{2,3})\s*[:|\-]\s*(.+)$").unwrap();
    if let Some(caps) = re_loose.captures(&cleaned) {
        let code = caps[1].to_uppercase();
        // Only accept real country codes — `XX:`, `ZZ:`, etc. are
        // not countries and should fall through (test expectation:
        // `XX: NEWS` → None).
        if country_code_to_name(&code).is_some() {
            return (Some(code), clean_category_suffix(&caps[2]));
        }
    }

    // 3. Full country name at start, followed by `:` or `|`.
    //    Catches `INDIA: HINDI NEWS`, `Pakistan | Drama`. Only matches
    //    when the prefix is a real country name (in the map), so
    //    `HINDI:` doesn't false-positive.
    let re_full = Regex::new(r"^([A-Z][A-Za-z ]{1,30})\s*[:|\-]\s*(.+)$").unwrap();
    if let Some(caps) = re_full.captures(&cleaned) {
        let candidate = caps[1].trim().to_uppercase();
        if let Some(code) = full_name_to_code(&candidate) {
            return (Some(code.to_string()), clean_category_suffix(&caps[2]));
        }
    }

    // 4. No prefix: try to match the whole category name against a known
    //    country (e.g. `HINDI INDIA` → India, `PAKISTAN NEWS` → Pakistan).
    if let Some((code, _)) = find_country_in_text(&cleaned) {
        return (Some(code), cleaned);
    }

    (None, cleaned)
}

/// Look for any country name or code mentioned anywhere in `text`.
/// Returns the first match as a 2-char ISO code.
#[allow(dead_code)]
fn find_country_in_text(text: &str) -> Option<(String, String)> {
    // Check 2-3 char codes first.
    let re_code = Regex::new(r"\b([A-Z]{2,3})\b").unwrap();
    for caps in re_code.captures_iter(text) {
        let code = caps[1].to_string();
        if country_code_to_name(&code).is_some() {
            return Some((code.clone(), country_code_to_name(&code).unwrap()));
        }
    }
    // Then check full country names.
    let names: Vec<(String, String)> = build_country_reverse_map();
    for (name, code) in &names {
        if text.to_uppercase().contains(name) {
            return Some((code.clone(), name.clone()));
        }
    }
    None
}

/// Build a (full_name, code) list from the country_code_to_name map.
#[allow(dead_code)]
fn build_country_reverse_map() -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    // Walk the known codes; for each, the name is the full English form.
    // We avoid duplicating by deduplicating on name.
    let codes: &[(&str, &str)] = &[
        ("AF", "AFGHANISTAN"),
        ("AL", "ALBANIA"),
        ("DZ", "ALGERIA"),
        ("AR", "ARGENTINA"),
        ("AM", "ARMENIA"),
        ("AU", "AUSTRALIA"),
        ("AT", "AUSTRIA"),
        ("AZ", "AZERBAIJAN"),
        ("BD", "BANGLADESH"),
        ("BY", "BELARUS"),
        ("BE", "BELGIUM"),
        ("BO", "BOLIVIA"),
        ("BA", "BOSNIA"),
        ("BR", "BRAZIL"),
        ("BG", "BULGARIA"),
        ("KH", "CAMBODIA"),
        ("CM", "CAMEROON"),
        ("CA", "CANADA"),
        ("CL", "CHILE"),
        ("CN", "CHINA"),
        ("CO", "COLOMBIA"),
        ("HR", "CROATIA"),
        ("CU", "CUBA"),
        ("CZ", "CZECH"),
        ("DK", "DENMARK"),
        ("EG", "EGYPT"),
        ("EE", "ESTONIA"),
        ("ET", "ETHIOPIA"),
        ("FI", "FINLAND"),
        ("FR", "FRANCE"),
        ("GE", "GEORGIA"),
        ("DE", "GERMANY"),
        ("GH", "GHANA"),
        ("GR", "GREECE"),
        ("HU", "HUNGARY"),
        ("IS", "ICELAND"),
        ("IN", "INDIA"),
        ("ID", "INDONESIA"),
        ("IR", "IRAN"),
        ("IQ", "IRAQ"),
        ("IE", "IRELAND"),
        ("IL", "ISRAEL"),
        ("IT", "ITALY"),
        ("JP", "JAPAN"),
        ("JO", "JORDAN"),
        ("KZ", "KAZAKHSTAN"),
        ("KE", "KENYA"),
        ("KR", "KOREA"),
        ("LB", "LEBANON"),
        ("LY", "LIBYA"),
        ("LT", "LITHUANIA"),
        ("MY", "MALAYSIA"),
        ("MX", "MEXICO"),
        ("MA", "MOROCCO"),
        ("NP", "NEPAL"),
        ("NL", "NETHERLANDS"),
        ("NZ", "NEW ZEALAND"),
        ("NG", "NIGERIA"),
        ("NO", "NORWAY"),
        ("PK", "PAKISTAN"),
        ("PE", "PERU"),
        ("PH", "PHILIPPINES"),
        ("PL", "POLAND"),
        ("PT", "PORTUGAL"),
        ("RO", "ROMANIA"),
        ("RU", "RUSSIA"),
        ("SA", "SAUDI ARABIA"),
        ("RS", "SERBIA"),
        ("SG", "SINGAPORE"),
        ("SK", "SLOVAKIA"),
        ("SI", "SLOVENIA"),
        ("ZA", "SOUTH AFRICA"),
        ("ES", "SPAIN"),
        ("LK", "SRI LANKA"),
        ("SE", "SWEDEN"),
        ("CH", "SWITZERLAND"),
        ("SY", "SYRIA"),
        ("TW", "TAIWAN"),
        ("TH", "THAILAND"),
        ("TR", "TURKEY"),
        ("UA", "UKRAINE"),
        ("AE", "UNITED ARAB EMIRATES"),
        ("GB", "UNITED KINGDOM"),
        ("US", "UNITED STATES"),
        ("UY", "URUGUAY"),
        ("VE", "VENEZUELA"),
        ("VN", "VIETNAM"),
        ("YE", "YEMEN"),
    ];
    let mut seen = std::collections::HashSet::new();
    for (code, name) in codes {
        if seen.insert(*code) {
            entries.push((name.to_string(), code.to_string()));
        }
    }
    entries
}

/// Reverse-lookup: given a full uppercase country name, return the
/// 2-char ISO code. Used by the full-name prefix matcher.
fn full_name_to_code(full_name: &str) -> Option<&'static str> {
    match full_name {
        "AFGHANISTAN" => Some("AF"),
        "ALBANIA" => Some("AL"),
        "ALGERIA" => Some("DZ"),
        "ARGENTINA" => Some("AR"),
        "ARMENIA" => Some("AM"),
        "AUSTRALIA" => Some("AU"),
        "AUSTRIA" => Some("AT"),
        "AZERBAIJAN" => Some("AZ"),
        "BANGLADESH" => Some("BD"),
        "BELARUS" => Some("BY"),
        "BELGIUM" => Some("BE"),
        "BOLIVIA" => Some("BO"),
        "BOSNIA" => Some("BA"),
        "BRAZIL" => Some("BR"),
        "BULGARIA" => Some("BG"),
        "CAMBODIA" => Some("KH"),
        "CAMEROON" => Some("CM"),
        "CANADA" => Some("CA"),
        "CHILE" => Some("CL"),
        "CHINA" => Some("CN"),
        "COLOMBIA" => Some("CO"),
        "CROATIA" => Some("HR"),
        "CUBA" => Some("CU"),
        "CZECH" => Some("CZ"),
        "DENMARK" => Some("DK"),
        "EGYPT" => Some("EG"),
        "ESTONIA" => Some("EE"),
        "ETHIOPIA" => Some("ET"),
        "FINLAND" => Some("FI"),
        "FRANCE" => Some("FR"),
        "GEORGIA" => Some("GE"),
        "GERMANY" => Some("DE"),
        "GHANA" => Some("GH"),
        "GREECE" => Some("GR"),
        "HUNGARY" => Some("HU"),
        "ICELAND" => Some("IS"),
        "INDIA" => Some("IN"),
        "INDONESIA" => Some("ID"),
        "IRAN" => Some("IR"),
        "IRAQ" => Some("IQ"),
        "IRELAND" => Some("IE"),
        "ISRAEL" => Some("IL"),
        "ITALY" => Some("IT"),
        "JAPAN" => Some("JP"),
        "JORDAN" => Some("JO"),
        "KAZAKHSTAN" => Some("KZ"),
        "KENYA" => Some("KE"),
        "KOREA" => Some("KR"),
        "LEBANON" => Some("LB"),
        "LIBYA" => Some("LY"),
        "LITHUANIA" => Some("LT"),
        "MALAYSIA" => Some("MY"),
        "MEXICO" => Some("MX"),
        "MOROCCO" => Some("MA"),
        "NEPAL" => Some("NP"),
        "NETHERLANDS" => Some("NL"),
        "NEW ZEALAND" => Some("NZ"),
        "NIGERIA" => Some("NG"),
        "NORWAY" => Some("NO"),
        "PAKISTAN" => Some("PK"),
        "PERU" => Some("PE"),
        "PHILIPPINES" => Some("PH"),
        "POLAND" => Some("PL"),
        "PORTUGAL" => Some("PT"),
        "ROMANIA" => Some("RO"),
        "RUSSIA" => Some("RU"),
        "SAUDI ARABIA" => Some("SA"),
        "SERBIA" => Some("RS"),
        "SINGAPORE" => Some("SG"),
        "SLOVAKIA" => Some("SK"),
        "SLOVENIA" => Some("SI"),
        "SOUTH AFRICA" => Some("ZA"),
        "SPAIN" => Some("ES"),
        "SRI LANKA" => Some("LK"),
        "SWEDEN" => Some("SE"),
        "SWITZERLAND" => Some("CH"),
        "SYRIA" => Some("SY"),
        "TAIWAN" => Some("TW"),
        "THAILAND" => Some("TH"),
        "TURKEY" => Some("TR"),
        "UKRAINE" => Some("UA"),
        "UNITED ARAB EMIRATES" => Some("AE"),
        "UNITED KINGDOM" => Some("GB"),
        "UNITED STATES" => Some("US"),
        "URUGUAY" => Some("UY"),
        "VENEZUELA" => Some("VE"),
        "VIETNAM" => Some("VN"),
        "YEMEN" => Some("YE"),
        _ => None,
    }
}

/// Given a country full name (any case), return the code if it matches
/// any known country. Used to validate candidate full-name prefixes
/// before accepting them.
#[allow(dead_code)]
fn country_code_to_name_by_full_name(full_upper: &str) -> Option<String> {
    let code = full_name_to_code(full_upper)?;
    country_code_to_name(code)
}

/// Strip quality tags like [720p], [1080p], [H265], (xxx) from a category name.
#[allow(dead_code)]
fn clean_category_suffix(name: &str) -> String {
    let re = Regex::new(r"\s*[\[\(][^\]\)]*[\]\)]\s*$").unwrap();
    let cleaned = re.replace(name, "").trim().to_string();
    if cleaned.is_empty() {
        name.trim().to_string()
    } else {
        cleaned
    }
}

/// Map a raw category name to a broad group: Sports, News, Entertainment, Kids, Movies, Music, Documentary, General.
#[allow(dead_code)]
pub fn categorize_group(name: &str) -> &'static str {
    let lower = name.to_uppercase();
    let keywords: &[(&[&str], &str)] = &[
        (
            &[
                "SPORT",
                "SPORTS",
                "FUTBOL",
                "FÚTBOL",
                "FOOTBALL",
                "SOCCER",
                "BEIN SPORTS",
                "ESPN",
                "DAZN",
                "BUNDESLIGA",
                "LALIGA",
                "LIGUE",
                "SERIE A",
                "PREMIER LEAGUE",
                "EPL",
                "NBA",
                "NFL",
                "NHL",
                "MLB",
                "MLS",
                "UFC",
                "BOXING",
                "TENNIS",
                "RUGBY",
                "GOLF",
                "CRICKET",
                "MOTOGP",
                "F1",
                "FORMULA",
                "NASCAR",
                "HOCKEY",
                "HANDBALL",
                "VOLLEY",
                "BASKETBALL",
                "WRESTLING",
                "SUPERCROSS",
                "MXGP",
                "RALLY",
                "SPORTOWE",
                "SPORTSKI",
                "DEPORTES",
                "DESPORTO",
                "SPORT DEUTSCHLAND",
                "DIREKTE SPORT",
                "CABEL TV SPORTS",
                "SPORTSNET",
                "EUROSPORT",
                "CYMRU FOOTBALL",
                "SCOTTISH FOOTBALL",
                "LA LIGA",
                "CHAMPIONSHIP",
                "LEAGUE ONE",
                "LEAGUE TWO",
                "NATIONAL LEAGUE",
                "HOCKEYETTAN",
                "SVENSK HOCKEY",
                "HANDBOLLSLIGAN",
                "INNEBANDY",
                "FLO RACING",
                "RACE",
                "RACING",
                "HORSE RACING",
                "PDC",
                "GLORY",
                "MATCHROOM",
                "TABII SPORT",
                "ODIDOSPORT",
                "MYSPORT",
                "MYTEAM SPORT",
                "OTROS DEPORTES",
                "REPETICIÓN",
                "REPLAY",
                "UEFA",
                "CUP GAMES",
                "WORLD CUP",
                "AHL",
                "QMJHL",
                "OHL",
                "WHL",
                "NIFL",
                "WNBA",
                "NCAAB",
                "NCAAF",
                "FLO COLLEGE",
                "FLO",
                "NFL PACKAGE",
                "NBA PACKAGE",
                "NHL PACKAGE",
                "MLB PACKAGE",
                "SVENSKBIL",
                "B1G+",
                "LNP PASS",
                "RIKSTOTO",
                "ALKASS",
                "L'EQUIPE",
                "ROGERS",
                "SPORTS NETWORK",
                "TENNIS CHANNEL",
                "VICTORY+",
                "WORLD SPORTS",
                "WOW SPORT",
                "ZIGGO SPORT",
                "VIAPLAY SPORT",
                "RTL+ SPORT",
                "MAGENTA",
                "MAX ESPN",
                "FOX",
                "STARZPLAY / AD",
                "SETANTA",
                "SN+",
                "PSF",
                "SFL",
                "DYN",
                "DYN NETWORK",
                "FANSEAT",
                "CLIPMYHORSE",
                "ULTIMATEPOOL",
                "LUXPLAY",
                "MYSPORT",
            ],
            "Sports",
        ),
        (
            &[
                "NEWS",
                "HABER",
                "NOTICIAS",
                "INFORMACYJNE",
                "REGIONALNE",
                "NEWS NETWORK",
                "GLOBO-RECORD-SBT-NEWS",
                "BBCI",
                "BBC STREAM",
                "INDIA NEWS",
                "AL-MAJD",
            ],
            "News",
        ),
        (
            &[
                "ENTERTAINMENT",
                "ENTRETENIMENTO",
                "INTRETENIMENTO",
                "DIVERTISMENT",
                "UNTERHALTUNG",
                "ALLGEMEIN",
                "ALGEMEEN",
                "GENERALISTAS",
                "ΓΕΝΙΚ",
                "CANALE GENERALE",
                "CANALE DE DIVERTISMENT",
                "REGIONAAL",
                "REGIONALI",
                "WOW ENTERTAINMENT",
                "ODIDO VERMAAK",
                "ESTILO DE VIDA",
                "FOREIGN",
                "LOCALES",
                "LOCALS",
                "AVRUPA",
                "YEREL",
                "REGIONAAL",
                "Buitenland",
            ],
            "Entertainment",
        ),
        (
            &[
                "KIDS",
                "KINDER",
                "KINDEREN",
                "KIDS NETWORK",
                "BAMBINI",
                "CRIANCAS",
                "CRIANÇAS",
                "ENFANTS",
                "DLA DZIECI",
                "COCUK",
                "ΠΑΙΔΙΚ",
                "INFANTIL",
                "CANALE PENTRU COPII",
                "EXYU: DECIJI",
                "EXYU: SKY KIDS",
                "DĚTI",
                "ZÁBAVA",
            ],
            "Kids",
        ),
        (
            &[
                "MOVIE",
                "MOVIES",
                "FILM",
                "FILMS",
                "FILME",
                "CINEMA",
                "CINEMANIA",
                "PELICULA",
                "SERIE",
                "SERIES",
                "DIZILER",
                "BOX OFFICE",
                "HBO",
                "MAX",
                "NETFLIX",
                "DISNEY",
                "AMAZON PRIME",
                "APPLE TV",
                "PARAMOUNT",
                "PEACOCK",
                "HULU",
                "TUBI",
                "PLEX",
                "PLUTO",
                "SHAHID",
                "CANAL+",
                "CANAL PLAY",
                "SKY GO",
                "SKY STORE",
                "VIAPLAY",
                "NOW TV",
                "RAKUTENTV",
                "MONOMAX",
                "M+ CINE",
                "MOVISTAR",
                "STARZ",
                "SHOWTIME",
                "OSN",
                "FOCUS SAT",
                "ZIGGO",
                "TELIA",
                "TV2 PLAY",
                "TV 4 PLAY",
                "PRIMA",
                "PRIME VIDEO",
                "DISCOVERY",
                "MAGENTA FILME",
                "SKY MAX",
                "COSMOTE",
                "MOLA",
                "KURDCINEMA",
                "SHAHID SERIES",
                "SHAHID. CINEMA",
                "CANALE DE CINEMA",
                "TDT SERIES",
                "SKYMIX SERIES",
                "CANALE DOCUMENTARE",
                "SKYMIX DOCS",
                "WATCH IT",
                "TIVIFY",
                "TIVIFY GOLD",
                "VIX",
                "REDBOX",
                "PLAY",
                "SCREEN TV",
                "CELCOM",
                "CELLCOMTV",
                "YES BOXES",
                "COMEDY",
                "DRAMA",
            ],
            "Movies & Series",
        ),
        (
            &[
                "MUSIC",
                "MUZIEK",
                "MUZIK",
                "MUZYCZNE",
                "MUSICA",
                "CANALE MUZICALE",
                "CULTURA",
                "STINGRAY",
                "MTV",
                "RADIO",
                "MUSIC CONCERTS",
            ],
            "Music",
        ),
        (
            &[
                "DOCUMENTAIRE",
                "DOCUMENTALES",
                "DOCUMENTARIO",
                "DOCUMENTARY",
                "DOKUMENTALNE",
                "BELGESEL",
                "DOCUMENTARE",
            ],
            "Documentary",
        ),
        (
            &[
                "RELIGIAO",
                "RELIGIOUS",
                "BIBLICAL",
                "ISLAMIC",
                "CHRISTIAN",
                "COCUK AND DINI",
                "DINI",
            ],
            "Religious",
        ),
    ];

    for (patterns, group) in keywords {
        for pat in *patterns {
            if lower.contains(pat) {
                return group;
            }
        }
    }
    "General"
}

/// Map a 2/3-letter ISO country code (or common abbreviation) to a full name.
/// Returns `None` if the code is unknown or already a full name.
pub fn country_code_to_name(code: &str) -> Option<String> {
    let upper = code.trim().to_uppercase();
    let name = match upper.as_str() {
        "AF" | "AFG" => "Afghanistan",
        "AL" | "ALB" => "Albania",
        "DZ" | "DZA" => "Algeria",
        "AD" | "AND" => "Andorra",
        "AO" | "AGO" => "Angola",
        "AR" | "ARG" => "Argentina",
        "AM" | "ARM" => "Armenia",
        "AU" | "AUS" => "Australia",
        "AT" | "AUT" => "Austria",
        "AZ" | "AZE" => "Azerbaijan",
        "BD" | "BGD" => "Bangladesh",
        "BY" | "BLR" => "Belarus",
        "BE" | "BEL" => "Belgium",
        "BT" | "BTN" => "Bhutan",
        "BO" | "BOL" => "Bolivia",
        "BA" | "BIH" => "Bosnia and Herzegovina",
        "BR" | "BRA" => "Brazil",
        "BN" | "BRN" => "Brunei",
        "BG" | "BGR" => "Bulgaria",
        "BF" | "BFA" => "Burkina Faso",
        "BI" | "BDI" => "Burundi",
        "KH" | "KHM" => "Cambodia",
        "CM" | "CMR" => "Cameroon",
        "CA" | "CAN" => "Canada",
        "CF" | "CAF" => "Central African Republic",
        "TD" | "TCD" => "Chad",
        "CL" | "CHL" => "Chile",
        "CN" | "CHN" => "China",
        "CO" | "COL" => "Colombia",
        "KM" | "COM" => "Comoros",
        "CG" | "COG" => "Congo",
        "CR" | "CRI" => "Costa Rica",
        "HR" | "HRV" => "Croatia",
        "CU" | "CUB" => "Cuba",
        "CY" | "CYP" => "Cyprus",
        "CZ" | "CZE" => "Czech Republic",
        "DK" | "DNK" => "Denmark",
        "DJ" | "DJI" => "Djibouti",
        "DO" | "DOM" => "Dominican Republic",
        "EC" | "ECU" => "Ecuador",
        "EG" | "EGY" => "Egypt",
        "SV" | "SLV" => "El Salvador",
        "GQ" | "GNQ" => "Equatorial Guinea",
        "ER" | "ERI" => "Eritrea",
        "EE" | "EST" => "Estonia",
        "ET" | "ETH" => "Ethiopia",
        "FI" | "FIN" => "Finland",
        "FR" | "FRA" => "France",
        "GA" | "GAB" => "Gabon",
        "GM" | "GMB" => "Gambia",
        "GE" | "GEO" => "Georgia",
        "DE" | "DEU" => "Germany",
        "GH" | "GHA" => "Ghana",
        "GR" | "GRC" => "Greece",
        "GT" | "GTM" => "Guatemala",
        "GN" | "GIN" => "Guinea",
        "GW" | "GNB" => "Guinea-Bissau",
        "HT" | "HTI" => "Haiti",
        "HN" | "HND" => "Honduras",
        "HK" | "HKG" => "Hong Kong",
        "HU" | "HUN" => "Hungary",
        "IS" | "ISL" => "Iceland",
        "IN" | "IND" => "India",
        "ID" | "IDN" => "Indonesia",
        "IR" | "IRN" => "Iran",
        "IQ" | "IRQ" => "Iraq",
        "IE" | "IRL" => "Ireland",
        "IL" | "ISR" => "Israel",
        "IT" | "ITA" => "Italy",
        "CI" | "CIV" => "Ivory Coast",
        "JM" | "JAM" => "Jamaica",
        "JP" | "JPN" => "Japan",
        "JO" | "JOR" => "Jordan",
        "KZ" | "KAZ" => "Kazakhstan",
        "KE" | "KEN" => "Kenya",
        "KR" | "KOR" => "South Korea",
        "KW" | "KWT" => "Kuwait",
        "KG" | "KGZ" => "Kyrgyzstan",
        "LA" | "LAO" => "Laos",
        "LV" | "LVA" => "Latvia",
        "LB" | "LBN" => "Lebanon",
        "LS" | "LSO" => "Lesotho",
        "LR" | "LBR" => "Liberia",
        "LY" | "LBY" => "Libya",
        "LT" | "LTU" => "Lithuania",
        "MO" | "MAC" => "Macau",
        "MK" | "MKD" => "North Macedonia",
        "MG" | "MDG" => "Madagascar",
        "MW" | "MWI" => "Malawi",
        "MY" | "MYS" => "Malaysia",
        "MV" | "MDV" => "Maldives",
        "ML" | "MLI" => "Mali",
        "MT" | "MLT" => "Malta",
        "MR" | "MRT" => "Mauritania",
        "MU" | "MUS" => "Mauritius",
        "MX" | "MEX" => "Mexico",
        "MD" | "MDA" => "Moldova",
        "MC" | "MCO" => "Monaco",
        "MN" | "MNG" => "Mongolia",
        "ME" | "MNE" => "Montenegro",
        "MA" | "MAR" => "Morocco",
        "MZ" | "MOZ" => "Mozambique",
        "MM" | "MMR" => "Myanmar",
        "NA" | "NAM" => "Namibia",
        "NP" | "NPL" => "Nepal",
        "NL" | "NLD" => "Netherlands",
        "NZ" | "NZL" => "New Zealand",
        "NI" | "NIC" => "Nicaragua",
        "NE" | "NER" => "Niger",
        "NG" | "NGA" => "Nigeria",
        "KP" | "PRK" => "North Korea",
        "NO" | "NOR" => "Norway",
        "OM" | "OMN" => "Oman",
        "PK" | "PAK" => "Pakistan",
        "PA" | "PAN" => "Panama",
        "PG" | "PNG" => "Papua New Guinea",
        "PY" | "PRY" => "Paraguay",
        "PE" | "PER" => "Peru",
        "PH" | "PHL" => "Philippines",
        "PL" | "POL" => "Poland",
        "PT" | "PRT" => "Portugal",
        "PR" | "PRI" => "Puerto Rico",
        "QA" | "QAT" => "Qatar",
        "RO" | "ROU" => "Romania",
        "RU" | "RUS" => "Russia",
        "RW" | "RWA" => "Rwanda",
        "SA" | "SAU" => "Saudi Arabia",
        "SN" | "SEN" => "Senegal",
        "RS" | "SRB" => "Serbia",
        "SC" | "SYC" => "Seychelles",
        "SL" | "SLE" => "Sierra Leone",
        "SG" | "SGP" => "Singapore",
        "SK" | "SVK" => "Slovakia",
        "SI" | "SVN" => "Slovenia",
        "SO" | "SOM" => "Somalia",
        "ZA" | "ZAF" => "South Africa",
        "SS" | "SSD" => "South Sudan",
        "ES" | "ESP" => "Spain",
        "LK" | "LKA" => "Sri Lanka",
        "SD" | "SDN" => "Sudan",
        "SR" | "SUR" => "Suriname",
        "SE" | "SWE" => "Sweden",
        "CH" | "CHE" => "Switzerland",
        "SY" | "SYR" => "Syria",
        "TW" | "TWN" => "Taiwan",
        "TJ" | "TJK" => "Tajikistan",
        "TZ" | "TZA" => "Tanzania",
        "TH" | "THA" => "Thailand",
        "TL" | "TLS" => "Timor-Leste",
        "TG" | "TGO" => "Togo",
        "TN" | "TUN" => "Tunisia",
        "TR" | "TUR" => "Turkey",
        "TM" | "TKM" => "Turkmenistan",
        "UG" | "UGA" => "Uganda",
        "UA" | "UKR" => "Ukraine",
        "AE" | "ARE" => "United Arab Emirates",
        "GB" | "UK" | "GBR" => "United Kingdom",
        "US" | "USA" => "United States",
        "UY" | "URY" => "Uruguay",
        "UZ" | "UZB" => "Uzbekistan",
        "VE" | "VEN" => "Venezuela",
        "VN" | "VNM" => "Vietnam",
        "YE" | "YEM" => "Yemen",
        "ZM" | "ZMB" => "Zambia",
        "ZW" | "ZWE" => "Zimbabwe",
        "Dubai" => "United Arab Emirates",
        _ => return None,
    };
    Some(name.to_string())
}

/// Normalize a country value: if it's a code, expand to full name; if already a
/// full name or unknown, return as-is.
pub fn normalize_country(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    country_code_to_name(trimmed).unwrap_or_else(|| trimmed.to_string())
}

/// Extract country from channel name using common patterns.
pub fn extract_country_from_name(name: &str) -> Option<String> {
    let re = Regex::new(r"(?i)\b(Pakistan|India|UK|USA|US|United Kingdom|United States|UAE|Dubai|Saudi|Turkey|France|Germany|Spain|Italy|Brazil|Mexico|Japan|Korea|China|Bangladesh|Sri Lanka|Nepal|Afghanistan|Iran|Iraq|Egypt|Nigeria|Kenya|South Africa|Australia|Canada|Indonesia|Thailand|Vietippines|Philippines|Malaysia|Singapore)\b").ok()?;
    let caps = re.captures(name)?;
    Some(caps[1].to_string())
}

/// Normalize channel name: trim, collapse whitespace, strip common prefixes.
pub fn normalize_channel_name(name: &str) -> String {
    let normalized = name.trim().split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        "Unknown Channel".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_server_url() {
        assert_eq!(
            normalize_server_url("https://example.com/").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            normalize_server_url("https://example.com:8080").unwrap(),
            "https://example.com:8080"
        );
        assert!(normalize_server_url("ftp://example.com").is_err());
        assert!(normalize_server_url("not-a-url").is_err());
        assert!(normalize_server_url("https://user:pass@example.com").is_err());
    }

    #[test]
    fn test_extract_country() {
        assert_eq!(
            extract_country_from_name("Geo News Pakistan"),
            Some("Pakistan".to_string())
        );
        assert_eq!(extract_country_from_name("BBC UK"), Some("UK".to_string()));
        assert_eq!(
            extract_country_from_name("CNN USA"),
            Some("USA".to_string())
        );
        assert_eq!(extract_country_from_name("Al Jazeera"), None);
    }

    #[test]
    fn test_normalize_channel_name() {
        assert_eq!(normalize_channel_name("  Geo News  "), "Geo News");
        assert_eq!(
            normalize_channel_name("CNN   International"),
            "CNN International"
        );
    }

    #[test]
    fn test_parse_category_name() {
        let (country, name) = parse_category_name("DK: SPORT [720p]");
        assert_eq!(country, Some("DK".to_string()));
        assert_eq!(name, "SPORT");

        let (country, name) = parse_category_name("USA: NEWS NETWORK");
        assert_eq!(country, Some("USA".to_string()));
        assert_eq!(name, "NEWS NETWORK");

        let (country, name) = parse_category_name("GENERAL");
        assert_eq!(country, None);
        assert_eq!(name, "GENERAL");

        let (country, name) = parse_category_name("UK: KIDS [H265]");
        assert_eq!(country, Some("UK".to_string()));
        assert_eq!(name, "KIDS");

        // 2-char codes that the old regex already handled.
        assert_eq!(parse_category_name("IN: NEWS").0, Some("IN".to_string()));
        assert_eq!(parse_category_name("PK: DRAMA").0, Some("PK".to_string()));
        assert_eq!(parse_category_name("AE: SPORTS").0, Some("AE".to_string()));

        // 2-char codes with `|` or `-` instead of `:`.
        assert_eq!(
            parse_category_name("IN | HINDI NEWS").0,
            Some("IN".to_string())
        );
        assert_eq!(parse_category_name("IN - HINDI").0, Some("IN".to_string()));
        assert_eq!(parse_category_name("IN-HINDI").0, Some("IN".to_string()));

        // Mixed-case codes.
        assert_eq!(parse_category_name("in: news").0, Some("IN".to_string()));
        assert_eq!(parse_category_name("In: News").0, Some("IN".to_string()));

        // Full country names as prefix (Xtream providers use these too).
        assert_eq!(
            parse_category_name("INDIA: HINDI NEWS").0,
            Some("IN".to_string())
        );
        assert_eq!(
            parse_category_name("INDIA | NEWS").0,
            Some("IN".to_string())
        );
        assert_eq!(
            parse_category_name("INDIA - ENTERTAINMENT").0,
            Some("IN".to_string())
        );
        assert_eq!(
            parse_category_name("Pakistan: News").0,
            Some("PK".to_string())
        );
        assert_eq!(
            parse_category_name("PAKISTAN | DRAMA").0,
            Some("PK".to_string())
        );
        assert_eq!(
            parse_category_name("UNITED STATES: CNN").0,
            Some("US".to_string())
        );
        assert_eq!(
            parse_category_name("United Kingdom | BBC").0,
            Some("GB".to_string())
        );
        assert_eq!(
            parse_category_name("SOUTH AFRICA: SPORTS").0,
            Some("ZA".to_string())
        );

        // No prefix but the full country name appears in the text.
        assert_eq!(parse_category_name("HINDI INDIA").0, Some("IN".to_string()));
        assert_eq!(
            parse_category_name("PAKISTAN NEWS").0,
            Some("PK".to_string())
        );
        assert_eq!(
            parse_category_name("BOLLYWOOD INDIA MOVIES").0,
            Some("IN".to_string())
        );

        // False positives avoided: a prefix that looks like a country code
        // but isn't one.
        assert_eq!(parse_category_name("XX: NEWS").0, None);
        // HINDI: NEWS has no real country indicator — the "HINDI" prefix
        // isn't a code, and "INDIA" doesn't appear as a separate word in
        // the text. Falls through to (None, "HINDI: NEWS").
        assert_eq!(parse_category_name("HINDI: NEWS").0, None);
    }

    #[test]
    fn test_categorize_group() {
        assert_eq!(categorize_group("SPORT"), "Sports");
        assert_eq!(categorize_group("NEWS NETWORK"), "News");
        assert_eq!(categorize_group("KIDS"), "Kids");
        assert_eq!(categorize_group("FILMS"), "Movies & Series");
        assert_eq!(categorize_group("MUSIC"), "Music");
        assert_eq!(categorize_group("DOCUMENTARY"), "Documentary");
        assert_eq!(categorize_group("GENERAL"), "General");
        assert_eq!(categorize_group("DK: BEIN SPORTS"), "Sports");
        assert_eq!(categorize_group("USA: HBO"), "Movies & Series");
    }
}
