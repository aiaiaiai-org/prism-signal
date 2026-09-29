// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: Apache-2.0

//! Fusion behaviour on real channel wording, through the real reader and the bridge.
//!
//! The all-clear cases here are the ones a first consumer found by replaying 495 live posts and
//! reading every all-clear it would have sent. They stay in the channel's own words: telling
//! someone a threat is over when it is not is the worst thing this layer can produce.

use prism_signal_bridge::{BridgeRules, bridge};
use prism_signal_core::{
    CellResolution, ConfidenceBand, Evidence, Geometry, Position, SignalObservation, Timestamp,
};
use prism_signal_fusion::{
    Assessed, Assessment, EventKind, FusionPolicy, HazardClass, Proximity, Status, assess,
};
use prism_signal_normalize::Normalizer;

fn ts(instant: &str) -> Timestamp {
    Timestamp::parse(instant).unwrap()
}

fn evidence(id: u64, published: &str, text: &str) -> Evidence {
    serde_json::from_value(serde_json::json!({
        "source_id": "telegram.channel:vanek_nikolaev",
        "external_id": format!("vanek_nikolaev/{id}"),
        "published_at": published,
        "text": text,
        "provenance": {"url": format!("https://t.me/vanek_nikolaev/{id}"), "collector": "test/0"}
    }))
    .unwrap()
}

fn observations(posts: &[(u64, &str, &str)]) -> Vec<SignalObservation> {
    let normalizer = Normalizer::embedded().unwrap();
    posts
        .iter()
        .flat_map(|(id, at, text)| {
            let readings = normalizer.read(&evidence(*id, at, text));
            bridge(&readings, &BridgeRules::default()).observations
        })
        .collect()
}

fn fuse(posts: &[(u64, &str, &str)], now: &str) -> Assessed {
    assess(&observations(posts), &FusionPolicy::v1(), ts(now))
}

fn of_class(assessed: &Assessed, class: HazardClass) -> Vec<&Assessment> {
    assessed
        .assessments
        .iter()
        .filter(|a| a.class == class)
        .collect()
}

fn at_place<'a>(assessed: &'a Assessed, name: &str) -> Vec<&'a Assessment> {
    assessed
        .assessments
        .iter()
        .filter(|a| a.place.as_ref().and_then(|p| p.name.as_deref()) == Some(name))
        .collect()
}

fn covers(assessment: &Assessment, lat: f64, lon: f64) -> bool {
    let own = prism_signal_geo::cover(
        &Geometry::point(Position::new(lon, lat).unwrap()),
        CellResolution::try_from(6).unwrap(),
        1,
    )
    .unwrap();
    assessment.cells.cells.contains(&own.cells[0])
}

const KYIV: (f64, f64) = (50.4501, 30.5234);
const LVIV: (f64, f64) = (49.8397, 24.0297);

// ---- lifecycle ----

#[test]
fn a_threat_is_issued_once_with_the_cells_a_person_must_stand_in() {
    let r = fuse(
        &[(1, "2026-09-28T22:19:42Z", "2 баллистики на Киев !")],
        "2026-09-28T22:20:00Z",
    );
    assert_eq!(r.assessments.len(), 1);
    let a = &r.assessments[0];
    assert_eq!(a.class, HazardClass::Missile);
    assert_eq!(a.status, Status::Active);
    assert_eq!(a.place.as_ref().unwrap().name.as_deref(), Some("Київ"));
    assert_eq!(
        a.assessment_id,
        "fusion.v1/missile/geonames:703448/vanek_nikolaev/1"
    );
    assert_eq!(
        a.valid_until,
        ts("2026-09-28T22:39:42Z"),
        "20 minutes for a missile"
    );
    assert_eq!(a.likelihood, ConfidenceBand::Moderate);
    assert_eq!(a.corroboration_count, 1);
    assert!(covers(a, KYIV.0, KYIV.1));
    assert!(covers(a, 50.40, 30.60));
    assert!(!covers(a, LVIV.0, LVIV.1));
    assert_eq!(r.events.len(), 1);
    assert_eq!((r.events[0].kind, r.events[0].seq), (EventKind::Issued, 1));
    assert_eq!(
        r.events[0].evidence.as_ref().unwrap().url,
        "https://t.me/vanek_nikolaev/1"
    );
}

#[test]
fn a_later_report_of_the_same_hazard_renews_it_instead_of_issuing_a_second() {
    let r = fuse(
        &[
            (1, "2026-09-28T22:19:42Z", "2 баллистики на Киев !"),
            (2, "2026-09-28T22:21:25Z", "ещё 2 баллистики на Киев !"),
            (
                3,
                "2026-09-28T22:24:00Z",
                "ещё вылеты Цирконов с курска курсом на Киев !",
            ),
        ],
        "2026-09-28T22:25:00Z",
    );
    assert_eq!(r.assessments.len(), 1);
    let a = &r.assessments[0];
    assert_eq!((a.revision, a.evidence.len()), (3, 3));
    assert_eq!(
        a.valid_until,
        ts("2026-09-28T22:44:00Z"),
        "extended by the newest report"
    );
    let kinds: Vec<_> = r.events.iter().map(|e| (e.kind, e.seq)).collect();
    assert_eq!(
        kinds,
        [
            (EventKind::Issued, 1),
            (EventKind::Superseded, 2),
            (EventKind::Superseded, 3)
        ]
    );
    assert_eq!(
        a.kinds.len(),
        2,
        "ballistic and other missiles are both named"
    );
}

#[test]
fn one_post_naming_a_hazard_twice_at_a_place_is_one_report() {
    // A generic `ракета` next to `баллистика` is one threat; nothing is renewed by a post that
    // is already the assessment's own evidence.
    let r = fuse(
        &[(
            1,
            "2026-09-28T22:19:42Z",
            "2 баллистики и 2 ракеты на Киев !",
        )],
        "2026-09-28T22:20:00Z",
    );
    assert_eq!(r.assessments.len(), 1);
    assert_eq!(r.events.len(), 1);
}

#[test]
fn a_window_that_ran_out_expires_and_the_next_report_is_a_new_assessment() {
    let posts = [
        (1, "2026-09-28T20:00:00Z", "1 реактивный мопед над Киевом"),
        (2, "2026-09-28T21:00:00Z", "1 реактивный мопед над Киевом"),
    ];
    let r = fuse(&posts, "2026-09-28T21:05:00Z");
    assert_eq!(r.assessments.len(), 2);
    assert_eq!(r.assessments[0].status, Status::Expired);
    assert_eq!(r.assessments[1].status, Status::Active);
    assert_ne!(
        r.assessments[0].assessment_id,
        r.assessments[1].assessment_id
    );
    let events: Vec<_> = r.events.iter().map(|e| e.kind).collect();
    assert_eq!(
        events,
        [EventKind::Issued, EventKind::Expired, EventKind::Issued]
    );
    let expired = &r.events[1];
    assert_eq!(
        expired.effective_at,
        ts("2026-09-28T20:30:00Z"),
        "when the window ran out"
    );
    assert!(expired.evidence.is_none());
}

#[test]
fn the_evaluation_time_decides_between_active_and_expired_and_hides_the_future() {
    let posts = [(1, "2026-09-28T22:00:00Z", "1 реактивный мопед над Киевом")];
    assert_eq!(
        fuse(&posts, "2026-09-28T22:29:00Z").assessments[0].status,
        Status::Active
    );
    let later = fuse(&posts, "2026-09-28T22:31:00Z");
    assert_eq!(later.assessments[0].status, Status::Expired);
    assert_eq!(later.events.last().unwrap().kind, EventKind::Expired);
    // Evaluated before the report was made, it does not exist yet.
    assert!(fuse(&posts, "2026-09-28T21:59:00Z").assessments.is_empty());
}

#[test]
fn different_classes_and_different_places_are_separate_assessments() {
    let r = fuse(
        &[(
            1,
            "2026-09-28T22:00:00Z",
            "1 реактивный мопед над Киевом\n\n2 баллистики на Одессу\n\nКАБы на Харьков",
        )],
        "2026-09-28T22:01:00Z",
    );
    assert_eq!(r.assessments.len(), 3);
    assert_eq!(of_class(&r, HazardClass::Drone).len(), 1);
    assert_eq!(of_class(&r, HazardClass::Bomb).len(), 1);
    assert_eq!(at_place(&r, "Одеса")[0].class, HazardClass::Missile);
}

#[test]
fn two_sources_corroborate_and_one_does_not() {
    let mut obs = observations(&[(1, "2026-09-28T22:00:00Z", "1 реактивный мопед над Киевом")]);
    let mut second = obs[0].clone();
    second.source_id = prism_signal_core::SourceId::new("telegram.channel", "other").unwrap();
    second.provenance.evidence_id =
        prism_signal_core::ExternalId::try_from("other/7".to_owned()).unwrap();
    second.observed_at = ts("2026-09-28T22:02:00Z");
    obs.push(second);
    let r = assess(&obs, &FusionPolicy::v1(), ts("2026-09-28T22:05:00Z"));
    assert_eq!(r.assessments.len(), 1);
    assert_eq!(r.assessments[0].corroboration_count, 2);
    assert_eq!(r.assessments[0].likelihood, ConfidenceBand::High);
}

// ---- retraction ----

#[test]
fn an_all_clear_for_that_hazard_at_that_place_retracts_it_and_a_new_threat_starts_afresh() {
    let r = fuse(
        &[
            (1, "2026-09-28T22:19:42Z", "1 реактивный мопед над Киевом"),
            (2, "2026-09-28T22:27:00Z", "минус по мопеду над Киевом"),
            (3, "2026-09-28T22:30:00Z", "1 реактивный мопед над Киевом"),
        ],
        "2026-09-28T22:31:00Z",
    );
    assert_eq!(r.assessments.len(), 2);
    assert_eq!(r.assessments[0].status, Status::Retracted);
    assert_eq!(r.assessments[1].status, Status::Active);
    let retraction = r
        .events
        .iter()
        .find(|e| e.kind == EventKind::Retracted)
        .unwrap();
    assert_eq!(retraction.assessment_id, r.assessments[0].assessment_id);
    assert_eq!(retraction.effective_at, ts("2026-09-28T22:27:00Z"));
    assert_eq!(
        retraction.reason,
        Some(prism_signal_fusion::RetractionReason::SourceAllClear)
    );
    assert_eq!(
        retraction.evidence.as_ref().unwrap().evidence_id.as_str(),
        "vanek_nikolaev/2"
    );
}

#[test]
fn an_all_clear_elsewhere_or_for_another_class_leaves_the_threat_standing() {
    let posts = [
        (1, "2026-09-28T22:19:42Z", "1 реактивный мопед над Киевом"),
        (2, "2026-09-28T22:21:00Z", "минус по мопеду на Ровно"),
        (3, "2026-09-28T22:22:00Z", "минус по КАБам на Киев"),
    ];
    let r = fuse(&posts, "2026-09-28T22:23:00Z");
    assert_eq!(r.assessments.len(), 1);
    assert_eq!(r.assessments[0].status, Status::Active);
}

#[test]
fn overlapping_footprints_are_not_enough_to_retract() {
    // Brovary lies inside Kyiv's footprint. Calling off the drone at Brovary must not call off
    // the drone at Kyiv, whose people may still be in danger.
    let r = fuse(
        &[
            (1, "2026-09-28T22:19:42Z", "1 реактивный мопед над Киевом"),
            (2, "2026-09-28T22:21:00Z", "минус по мопеду над Броварами"),
        ],
        "2026-09-28T22:22:00Z",
    );
    assert_eq!(at_place(&r, "Київ")[0].status, Status::Active);
}

#[test]
fn an_all_clear_never_reaches_backwards_or_ends_a_later_threat() {
    let r = fuse(
        &[
            (1, "2026-09-28T22:10:00Z", "минус по мопеду над Киевом"),
            (2, "2026-09-28T22:20:00Z", "1 реактивный мопед над Киевом"),
        ],
        "2026-09-28T22:21:00Z",
    );
    assert_eq!(r.assessments.len(), 1);
    assert_eq!(r.assessments[0].status, Status::Active);
}

#[test]
fn an_all_clear_after_the_window_ran_out_retracts_nothing() {
    let r = fuse(
        &[
            (1, "2026-09-28T20:00:00Z", "1 реактивный мопед над Киевом"),
            (2, "2026-09-28T21:00:00Z", "минус по мопеду над Киевом"),
        ],
        "2026-09-28T21:05:00Z",
    );
    assert_eq!(
        r.assessments[0].status,
        Status::Expired,
        "it lapsed, it was not called off"
    );
}

#[test]
fn a_clear_contradicted_by_its_own_post_is_ignored() {
    let r = fuse(
        &[
            (1, "2026-09-28T22:19:42Z", "1 реактивный мопед над Киевом"),
            (
                2,
                "2026-09-28T22:25:00Z",
                "минус по мопеду над Киевом\n\nещё 1 мопед над Киевом",
            ),
        ],
        "2026-09-28T22:26:00Z",
    );
    assert!(r.assessments.iter().all(|a| a.status != Status::Retracted));
}

#[test]
fn an_all_clear_for_one_place_in_a_post_that_reports_the_hazard_elsewhere_still_retracts_there() {
    // Over Kyiv: minus. A drone near Bila Tserkva is somewhere else.
    let r = fuse(
        &[
            (1, "2026-09-28T22:19:42Z", "1 реактивный мопед над Киевом"),
            (
                2,
                "2026-09-28T22:27:00Z",
                "по мопедам над Киевом на сейчас минуса\n\n1 реактивный мопед пролетел в районе Белой Церкви в сторону Василькова/Фастова",
            ),
        ],
        "2026-09-28T22:28:00Z",
    );
    assert_eq!(at_place(&r, "Київ")[0].status, Status::Retracted);
    assert_eq!(at_place(&r, "Біла Церква")[0].status, Status::Active);
    assert_eq!(at_place(&r, "Фастів")[0].status, Status::Active);
}

// ---- the false all-clears found on real data ----
//
// Each of these is written the way the channel writes it. None may end an assessment.

fn standing_after(second_post: &str) -> Status {
    let r = fuse(
        &[
            (1, "2026-09-28T22:19:42Z", "2 баллистики на Киев !"),
            (2, "2026-09-28T22:25:00Z", second_post),
        ],
        "2026-09-28T22:26:00Z",
    );
    at_place(&r, "Київ")[0].status
}

#[test]
fn until_the_all_clear_is_not_the_all_clear() {
    assert_eq!(
        standing_after("угроза баллистики с брянска актуальна до отбоя тревоги на Киев"),
        Status::Active
    );
}

#[test]
fn an_interception_is_not_the_all_clear() {
    assert_eq!(
        standing_after("баллистика на Киев, есть уже первые сбития"),
        Status::Active
    );
}

#[test]
fn a_lost_track_is_not_the_all_clear() {
    assert_eq!(
        standing_after("баллистика на Киев больше не фиксируется"),
        Status::Active
    );
}

#[test]
fn an_all_clear_that_names_no_place_is_not_evidence_and_the_threat_lapses_on_its_window() {
    // "минус по ракетам ... угроза баллистики пока актуальна": no place to apply it to.
    let posts = [
        (1, "2026-09-28T22:19:42Z", "2 баллистики на Киев !"),
        (
            2,
            "2026-09-28T22:27:00Z",
            "на сейчас минус по ракетам, что писал выше\n\nвсе эти пуски были с курской губернии\n\nмогут пустить остальные, угроза баллистики/противокорабельных ракет пока актуальна",
        ),
    ];
    let r = fuse(&posts, "2026-09-28T22:28:00Z");
    assert_eq!(r.assessments.len(), 1);
    assert_eq!(r.assessments[0].status, Status::Active);
    // It lapses on schedule instead.
    let later = fuse(&posts, "2026-09-28T23:00:00Z");
    assert_eq!(later.assessments[0].status, Status::Expired);
}

#[test]
fn a_kind_only_all_clear_is_left_to_expire_the_price_of_being_safe() {
    let r = fuse(
        &[
            (1, "2026-09-28T22:19:42Z", "1 реактивный мопед над Киевом"),
            (
                2,
                "2026-09-28T22:27:00Z",
                "минус по всем этим 4 реактивным мопедам",
            ),
        ],
        "2026-09-28T22:28:00Z",
    );
    assert_eq!(r.assessments[0].status, Status::Active);
}

// ---- determinism and bounds ----

#[test]
fn the_same_window_gives_the_same_result_in_any_order() {
    let posts = [
        (
            1,
            "2026-09-28T22:19:42Z",
            "1 реактивный мопед над Киевом\n\n2 баллистики на Одессу",
        ),
        (
            2,
            "2026-09-28T22:21:00Z",
            "ещё 1 реактивный мопед над Киевом",
        ),
        (3, "2026-09-28T22:27:00Z", "минус по мопеду над Киевом"),
        (4, "2026-09-28T22:30:00Z", "КАБы на Харьков"),
    ];
    let forward = observations(&posts);
    let mut reversed = forward.clone();
    reversed.reverse();
    let policy = FusionPolicy::v1();
    let now = ts("2026-09-28T22:35:00Z");
    let a = assess(&forward, &policy, now);
    assert_eq!(a, assess(&reversed, &policy, now));
    assert_eq!(a, assess(&forward, &policy, now));
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&assess(&reversed, &policy, now)).unwrap()
    );
}

#[test]
fn an_empty_or_fully_future_window_gives_nothing_not_a_default() {
    let r = assess(&[], &FusionPolicy::v1(), ts("2026-09-28T22:00:00Z"));
    assert_eq!(r, Assessed::default());
}

#[test]
fn a_bare_point_gets_a_disc_and_an_unusable_geometry_is_reported_not_guessed() {
    let mut obs = observations(&[(1, "2026-09-28T22:00:00Z", "1 реактивный мопед над Киевом")]);
    obs[0].geometry = Geometry::point(Position::new(KYIV.1, KYIV.0).unwrap());
    let r = assess(&obs, &FusionPolicy::v1(), ts("2026-09-28T22:01:00Z"));
    assert_eq!(r.assessments.len(), 1);
    assert!(covers(&r.assessments[0], KYIV.0, KYIV.1));

    let mut broken = obs[0].clone();
    broken.geometry = Geometry::Cells {
        resolution: CellResolution::try_from(6).unwrap(),
        cells: vec![],
    };
    let r = assess(&[broken], &FusionPolicy::v1(), ts("2026-09-28T22:01:00Z"));
    assert!(r.assessments.is_empty());
    assert_eq!(r.skipped[0].code, "invalid_geometry");

    let mut tiny = FusionPolicy::v1();
    tiny.max_cells = 2;
    let r = assess(&obs, &tiny, ts("2026-09-28T22:01:00Z"));
    assert!(r.assessments.is_empty());
    assert_eq!(r.skipped[0].code, "cover_too_large");
}

#[test]
fn an_unknown_policy_is_refused() {
    assert!(FusionPolicy::named("fusion.v1").is_ok());
    assert_eq!(FusionPolicy::named("fusion.v9").unwrap_err().0, "fusion.v9");
}

#[test]
fn a_comma_joined_all_clear_and_threat_does_not_end_the_threat() {
    // Found by reading every retraction the pipeline made on 495 live posts: "minus" was for the
    // first two, and two more were on their way to Chornomorsk.
    let r = fuse(
        &[
            (
                1,
                "2026-09-04T01:55:00Z",
                "2 КАБа (УМПБ-5) летят к Черноморску",
            ),
            (
                2,
                "2026-09-04T02:04:00Z",
                "по первым 2 КАБам минус, еще 2 КАБа подлетают (пару минут) к Черноморску",
            ),
        ],
        "2026-09-04T02:05:00Z",
    );
    let at = at_place(&r, "Чорноморськ");
    assert_eq!(at.len(), 1);
    assert_eq!(
        at[0].status,
        Status::Active,
        "the second pair is still coming"
    );
    assert_eq!(
        at[0].revision, 2,
        "and it renewed the alert instead of ending it"
    );
}

// ---- proximity ----

#[test]
fn a_hazard_only_passing_a_place_is_nearby_until_a_report_aims_it_there() {
    let r = fuse(
        &[(
            1,
            "2026-09-28T22:00:00Z",
            "1 реактивный мопед пролетает южнее Каменского",
        )],
        "2026-09-28T22:01:00Z",
    );
    assert_eq!(at_place(&r, "Кам'янське")[0].proximity, Proximity::Nearby);

    let aimed = fuse(
        &[
            (
                1,
                "2026-09-28T22:00:00Z",
                "1 реактивный мопед пролетает южнее Каменского",
            ),
            (
                2,
                "2026-09-28T22:03:00Z",
                "1 реактивный мопед курсом на Каменское",
            ),
        ],
        "2026-09-28T22:04:00Z",
    );
    let a = at_place(&aimed, "Кам'янське");
    assert_eq!(a.len(), 1, "the same episode");
    assert_eq!(
        a[0].proximity,
        Proximity::Target,
        "upgraded, and renewed so a consumer hears of it"
    );
    assert_eq!(a[0].revision, 2);
}

#[test]
fn a_hazard_aimed_at_a_place_is_a_target_and_stays_one() {
    let r = fuse(
        &[
            (1, "2026-09-28T22:00:00Z", "1 реактивный мопед над Киевом"),
            (
                2,
                "2026-09-28T22:02:00Z",
                "1 реактивный мопед пролетает мимо Киева",
            ),
        ],
        "2026-09-28T22:03:00Z",
    );
    assert_eq!(at_place(&r, "Київ")[0].proximity, Proximity::Target);
}

#[test]
fn a_reader_that_gives_no_role_is_read_as_aiming_at_the_place() {
    let mut obs = observations(&[(
        1,
        "2026-09-28T22:00:00Z",
        "1 реактивный мопед пролетает южнее Каменского",
    )]);
    for o in &mut obs {
        o.provenance.place_role = None;
    }
    let r = assess(&obs, &FusionPolicy::v1(), ts("2026-09-28T22:01:00Z"));
    assert_eq!(r.assessments[0].proximity, Proximity::Target);
}
