use desktop_pet_contracts::{AgentOutcome, SessionKey, SourceCounts, MAX_COUNT};
use serde_json::json;

#[test]
fn session_key_retains_serialized_fields_and_rejects_unknown_authority() {
    let key = SessionKey {
        source_id: 7,
        generation: 3,
        terminal_id: "terminal".into(),
    };
    let encoded = serde_json::to_value(&key).unwrap();
    assert_eq!(
        encoded,
        json!({ "source_id": 7, "generation": 3, "terminal_id": "terminal" })
    );
    assert_eq!(serde_json::from_value::<SessionKey>(encoded).unwrap(), key);
    assert!(serde_json::from_value::<SessionKey>(json!({
        "source_id": 7, "generation": 3, "terminal_id": "terminal", "provider": "fake"
    }))
    .is_err());
}

#[test]
fn disconnect_keeps_bounded_session_count_and_removes_active_status() {
    let counts = SourceCounts {
        sessions: usize::MAX,
        working: 12,
        blocked: 3,
        done: 5,
        unknown: 2,
    };
    assert_eq!(
        counts.disconnected(),
        SourceCounts {
            sessions: MAX_COUNT as usize,
            working: 0,
            blocked: 0,
            done: 0,
            unknown: MAX_COUNT as usize,
        }
    );
    assert_eq!(counts.bounded().sessions, MAX_COUNT as usize);
    assert_eq!(counts.bounded().working, 12);
}

#[test]
fn normalized_outcomes_do_not_confuse_running_with_terminal() {
    assert!(!AgentOutcome::Running.is_terminal());
    for outcome in [
        AgentOutcome::Succeeded,
        AgentOutcome::Failed,
        AgentOutcome::Cancelled,
    ] {
        assert!(outcome.is_terminal());
    }
}
