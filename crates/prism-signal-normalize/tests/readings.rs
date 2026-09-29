// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Rule-by-rule behaviour of the normalizer on small texts.

use prism_signal_core::Evidence;
use prism_signal_normalize::{HazardKind, Normalizer, Phase, PlaceRole, Reading};

fn post(text: &str) -> Evidence {
    serde_json::from_value(serde_json::json!({
        "source_id": "telegram.channel:vanek_nikolaev",
        "external_id": "vanek_nikolaev/1",
        "published_at": "2026-09-29T12:00:00Z",
        "text": text,
        "provenance": {"url": "https://t.me/vanek_nikolaev/1", "collector": "test/0"}
    }))
    .unwrap()
}

fn read(text: &str) -> Vec<Reading> {
    Normalizer::embedded().unwrap().read(&post(text))
}

/// `Name:role` pairs of a reading, in reading order.
fn places(reading: &Reading) -> Vec<(&str, PlaceRole)> {
    reading
        .places
        .iter()
        .map(|p| (p.name.as_str(), p.role))
        .collect()
}

#[test]
fn a_launch_origin_is_not_a_target() {
    let readings = read("1 реактивный мопед подлетает к Киеву со стороны Гостомеля");
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].kind, Some(HazardKind::Drone));
    assert_eq!(readings[0].phase, Phase::Threat);
    assert_eq!(
        places(&readings[0]),
        [
            ("Київ", PlaceRole::Target),
            ("Гостомель", PlaceRole::Origin)
        ]
    );
    assert!(readings[0].actionable());
    let affected: Vec<_> = readings[0].affected().map(|p| p.name.as_str()).collect();
    assert_eq!(affected, ["Київ"]);
}

#[test]
fn a_place_already_passed_is_context_and_the_next_one_is_the_target() {
    let readings = read("3 реактивных мопеда пролетели Киев дальше в сторону Фастова");
    assert_eq!(
        places(&readings[0]),
        [("Київ", PlaceRole::Mention), ("Фастів", PlaceRole::Target)]
    );
}

#[test]
fn list_items_share_the_role_of_the_first() {
    let readings = read("2 реактивных мопеда опять развернулись к Киеву/Ирпеню, Буче и Броварам");
    assert_eq!(
        places(&readings[0]),
        [
            ("Київ", PlaceRole::Target),
            ("Ірпінь", PlaceRole::Target),
            ("Буча", PlaceRole::Target),
            ("Бровари", PlaceRole::Target),
        ]
    );
}

#[test]
fn an_either_or_cue_takes_the_strongest_role() {
    let readings = read("1 реактивный мопед курсом на/через Авангард/Одессу");
    assert_eq!(
        places(&readings[0]),
        [
            ("Авангард", PlaceRole::Target),
            ("Одеса", PlaceRole::Target)
        ]
    );
}

#[test]
fn passing_and_nearby_cues_give_via() {
    for (text, expected) in [
        ("мопед пролетает южнее Каменского", "Кам'янське"),
        ("мопед крутится возле Кропивницкого", "Кропивницький"),
        ("мопед под Борисполем", "Бориспіль"),
        ("мопед пролетают Черноморск", "Чорноморськ"),
    ] {
        let readings = read(text);
        assert_eq!(places(&readings[0]), [(expected, PlaceRole::Via)], "{text}");
    }
    let between = read("2 реактивных мопеда между Киевом и Васильковом");
    assert_eq!(
        places(&between[0]),
        [("Київ", PlaceRole::Via), ("Васильків", PlaceRole::Via)]
    );
}

#[test]
fn a_filler_word_does_not_break_the_cue() {
    let readings = read("1 реактивный мопед над центром Николаева");
    assert_eq!(places(&readings[0]), [("Миколаїв", PlaceRole::Target)]);
}

#[test]
fn a_place_without_a_cue_is_only_mentioned() {
    let readings =
        read("1 реактивный мопед подлетает к пункту пропуска на границе с Польшей Ягодин");
    assert_eq!(places(&readings[0]), [("Ягодин", PlaceRole::Mention)]);
    assert!(!readings[0].actionable());
}

#[test]
fn each_line_and_sentence_is_read_on_its_own() {
    let readings = read("1 реактивный мопед над Киевом\n\n1 реактивный мопед над Днепром");
    assert_eq!(readings.len(), 2);
    assert_eq!(places(&readings[0]), [("Київ", PlaceRole::Target)]);
    assert_eq!(places(&readings[1]), [("Дніпро", PlaceRole::Target)]);
}

#[test]
fn several_kinds_in_one_sentence_give_one_reading_each() {
    let readings = read("мопеды и КАБы на Киев");
    let kinds: Vec<_> = readings.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [Some(HazardKind::Drone), Some(HazardKind::GuidedBomb)]
    );
}

#[test]
fn a_generic_missile_is_absorbed_by_a_specific_one() {
    let readings = read("угроза баллистики/противокорабельных ракет пока актуальна");
    let kinds: Vec<_> = readings.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Some(HazardKind::BallisticMissile)]);
    let generic = read("2 ракеты на Киев");
    assert_eq!(generic[0].kind, Some(HazardKind::Missile));
}

#[test]
fn specific_kinds_are_recognised_in_both_languages() {
    for (text, kind) in [
        ("шахеди на Київ", HazardKind::Drone),
        ("БпЛА на Київ", HazardKind::Drone),
        ("КАБи на Харків", HazardKind::GuidedBomb),
        ("пуски КАБ (УМПБ-5) на Затоку", HazardKind::GuidedBomb),
        ("Калибры на Одессу", HazardKind::CruiseMissile),
        (
            "2 Циркона/Оникс-М подлетают к Киеву",
            HazardKind::CruiseMissile,
        ),
        ("2 баллистики на Киев", HazardKind::BallisticMissile),
        ("балістика на Дніпро", HazardKind::BallisticMissile),
    ] {
        let readings = read(text);
        assert_eq!(readings[0].kind, Some(kind), "{text}");
        assert!(readings[0].actionable(), "{text}");
    }
}

#[test]
fn an_all_clear_word_calls_the_threat_off() {
    for text in [
        "минус по всем этим 4 реактивным мопедам",
        "минуса по обоим этим мопедам",
        "мінус по мопедах",
        "отбой по мопедам",
    ] {
        let readings = read(text);
        assert_eq!(readings.len(), 1, "{text}");
        assert_eq!(readings[0].phase, Phase::Cleared, "{text}");
        assert_eq!(readings[0].kind, Some(HazardKind::Drone), "{text}");
        assert!(!readings[0].actionable(), "{text}");
    }
}

#[test]
fn a_bare_all_clear_has_no_kind_and_keeps_its_places() {
    let readings = read("минус по всему на Маяки или Одессу");
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].phase, Phase::Cleared);
    assert_eq!(readings[0].kind, None);
    assert_eq!(
        places(&readings[0]),
        [("Маяки", PlaceRole::Target), ("Одеса", PlaceRole::Target)]
    );
}

#[test]
fn only_explicit_all_clear_words_end_a_threat() {
    // No further launches, a target no longer tracked, some interceptions: each says nothing
    // about what is still in the air, and a message saying "all clear" would be false comfort.
    for text in [
        "повторных пусков КАБ больше не было",
        "ракета больше не фиксируется",
        "все ракеты летят на Черкассы, есть уже первые сбития",
        "часть мопедов сбита над Киевом",
    ] {
        assert!(
            read(text).iter().all(|r| r.phase == Phase::Threat),
            "{text}"
        );
    }
}

#[test]
fn until_the_all_clear_is_not_the_all_clear() {
    for text in [
        "угроза баллистики с брянска актуальна до отбоя тревоги",
        "угроза баллистики актуальна до відбою",
        "баллистика на Киев, отбоя пока нет",
        "баллистика на Киев, без отбоя",
    ] {
        let readings = read(text);
        assert!(!readings.is_empty(), "{text}");
        assert!(readings.iter().all(|r| r.phase == Phase::Threat), "{text}");
    }
    // The all-clear itself, in the same words, still counts.
    assert_eq!(read("минус по баллистике")[0].phase, Phase::Cleared);
    assert_eq!(read("по баллистике отбой")[0].phase, Phase::Cleared);
}

#[test]
fn an_all_clear_phrase_is_supported_but_needs_a_stated_kind() {
    // The shipped lexicon has no phrases; a custom one shows the rule they follow.
    let mut lexicon: serde_json::Value =
        serde_json::from_str(include_str!("../data/lexicon.v1.json")).unwrap();
    lexicon["cleared"]["phrases"] = serde_json::json!(["больше не"]);
    let custom = Normalizer::from_vocabulary(
        prism_signal_normalize::Gazetteer::embedded().unwrap(),
        &lexicon.to_string(),
    )
    .unwrap();

    let cleared = custom.read(&post("повторных пусков КАБ больше не было"));
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0].phase, Phase::Cleared);
    assert_eq!(cleared[0].kind, Some(HazardKind::GuidedBomb));
    // Without a kind, a weak phrase says nothing at all.
    assert!(custom.read(&post("я больше не могу молчать")).is_empty());
}

#[test]
fn a_kind_is_carried_to_the_places_listed_after_it() {
    let readings = read(
        "общая по мопедам:\n\n1 под Киевом\n\n1 над Днепром\n\n2 курсом на/через Гостомель/Киев",
    );
    let located: Vec<_> = readings.iter().filter(|r| r.actionable()).collect();
    assert_eq!(located.len(), 3);
    assert!(located.iter().all(|r| r.kind == Some(HazardKind::Drone)));
    assert!(located.iter().all(|r| r.kind_inherited));
    assert!(!readings[0].kind_inherited);

    let split = read("ещё +2 бандероли, то есть уже 8\n\nэти летят на Кривой Рог - будет громко!");
    assert_eq!(split.len(), 2);
    assert_eq!(split[1].kind, Some(HazardKind::Missile));
    assert!(split[1].kind_inherited);
    assert_eq!(places(&split[1]), [("Кривий Ріг", PlaceRole::Target)]);
}

#[test]
fn a_place_in_unrelated_prose_does_not_inherit_the_kind() {
    // The reviewed case: a cue and a place are not enough to continue a threat.
    let readings = read("1 мопед на Киев\nПВО в Киеве работает");
    assert_eq!(readings.len(), 1);
    assert_eq!(places(&readings[0]), [("Київ", PlaceRole::Target)]);
    assert!(!readings[0].kind_inherited);

    // A leading number alone is not a list: the earlier sentence already had its own place.
    let counted = read("1 мопед на Киев\n3 взрыва в Киеве");
    assert_eq!(counted.len(), 1);

    // A header with no place opens a list only for items that are counted.
    let header = read("общая по мопедам:\nПВО в Киеве работает");
    assert_eq!(header.len(), 1);
    assert!(header[0].places.is_empty());
    assert!(!header.iter().any(Reading::actionable));

    // `все` alone is not a reference: it is common in ordinary sentences.
    let everyone = read("1 мопед над Киевом\nвсе в Киеве спокойны");
    assert_eq!(everyone.len(), 1);

    // Nor does a number that is not an item: it must open the sentence.
    let mid = read("общая по мопедам:\nв Киеве слышны 3 взрыва");
    assert!(!mid.iter().any(Reading::actionable));
}

#[test]
fn a_sentence_that_refers_back_continues_the_threat() {
    for text in [
        "1 мопед над Киевом\nещё один летит к Фастову",
        "1 мопед над Киевом\nэти летят на Днепр",
        "1 мопед над Киевом\nостальные подлетают к Броварам",
        "1 мопед над Киевом\nвсе остальные летят на Днепр",
        "1 мопед над Киевом\nможет быть громко в Николаеве!",
    ] {
        let readings = read(text);
        assert_eq!(readings.len(), 2, "{text}");
        assert_eq!(readings[1].kind, Some(HazardKind::Drone), "{text}");
        assert!(readings[1].kind_inherited, "{text}");
        assert!(readings[1].actionable(), "{text}");
    }
}

#[test]
fn an_all_clear_ends_the_carry_over() {
    let readings = read("минус по мопедам\n\n1 над Киевом");
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].phase, Phase::Cleared);
}

#[test]
fn unknown_capitalised_targets_are_reported_never_guessed() {
    let readings = read("3 бандероли летят в сторону Виноградара Одесской области");
    assert_eq!(readings.len(), 1);
    assert!(readings[0].places.is_empty());
    assert_eq!(readings[0].unresolved, ["Виноградара", "Одесской"]);
    assert!(!readings[0].actionable());

    // Where a launch came from is not a candidate for the gazetteer.
    let origin = read("пуски баллистики с Курска");
    assert!(origin[0].unresolved.is_empty());
}

#[test]
fn regions_and_common_words_are_not_read_as_places() {
    assert!(read("мопеды над Николаевщиной")[0].places.is_empty());
    assert!(read("2 мопеда летят ровно на север")[0].places.is_empty());
    assert!(read("мопеды летят на маяки")[0].places.is_empty());
}

#[test]
fn text_without_a_hazard_gives_no_reading() {
    assert!(read("Російський удар проти будівлі в Києві. Триває рятувальна операція.").is_empty());
    assert!(read("Миколаїв: тихо 😌").is_empty());
}

#[test]
fn long_posts_are_summaries_not_live_alerts() {
    let long = format!("мопеды на Киев {}", "слово ".repeat(120));
    assert!(read(&long).is_empty());
    let normalizer = Normalizer::embedded().unwrap().max_text_chars(2000);
    assert_eq!(normalizer.read(&post(&long)).len(), 1);
}

#[test]
fn media_only_posts_give_no_reading() {
    let mut evidence = post("");
    evidence.text = None;
    assert!(Normalizer::embedded().unwrap().read(&evidence).is_empty());
}

#[test]
fn a_forwarded_post_is_flagged_not_dropped() {
    let mut evidence = post("мопеды на Киев");
    evidence.forwarded_from = Some("Другий канал".to_owned());
    let readings = Normalizer::embedded().unwrap().read(&evidence);
    assert!(readings[0].forwarded);
    assert!(!read("мопеды на Киев")[0].forwarded);
}

#[test]
fn readings_are_deterministic_and_round_trip_through_json() {
    let text = "2 баллистики на Запорожье/Кривой Рог\n\nещё 1 мопед возле Полтавы";
    let first = read(text);
    assert_eq!(first, read(text));

    let json = serde_json::to_string(&first[0]).unwrap();
    assert!(json.contains(r#""kind":"ballistic_missile""#));
    assert!(json.contains(r#""phase":"threat""#));
    assert!(json.contains(r#""role":"target""#));
    assert!(json.contains(r#""place_id":"geonames:687700""#));
    assert!(json.contains(r#""url":"https://t.me/vanek_nikolaev/1""#));
    assert!(!json.contains("kind_inherited"));
    assert!(!json.contains("unresolved"));
    let back: Reading = serde_json::from_str(&json).unwrap();
    assert_eq!(back, first[0]);
}
