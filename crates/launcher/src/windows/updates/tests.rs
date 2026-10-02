use super::*;
const TEST_KEY: &[u8; 64] = include_bytes!("fixtures/test-key.bin");
const TEST_MANIFEST: &str = include_str!("fixtures/manifest.txt");

#[test]
fn dotnet_p1363_signature_verifies_with_cng_and_rejects_tampering() {
    let manifest = Manifest::verify_with_key(TEST_MANIFEST.as_bytes(), TEST_KEY).unwrap();
    assert_eq!(manifest.version.to_string(), "9.8.7");
    staging::check_hash(b"test payload", &manifest).unwrap();
    assert!(matches!(
        staging::check_hash(b"changed payload", &manifest),
        Err(UpdateError::HashMismatch)
    ));
    assert!(matches!(
        Manifest::verify(TEST_MANIFEST.as_bytes()),
        Err(UpdateError::BadSignature)
    ));
    for text in [
        TEST_MANIFEST.replace("9.8.7", "9.8.8"),
        TEST_MANIFEST.replace("sha256=", &format!("sha256={}", "0".repeat(64))),
    ] {
        assert!(Manifest::verify_with_key(text.as_bytes(), TEST_KEY).is_err());
    }
    let mut signature_changed = TEST_MANIFEST.to_owned();
    let offset = signature_changed.find("signature=").unwrap() + "signature=".len();
    let replacement = if &signature_changed[offset..offset + 1] == "A" {
        "B"
    } else {
        "A"
    };
    signature_changed.replace_range(offset..offset + 1, replacement);
    assert!(matches!(
        Manifest::verify_with_key(signature_changed.as_bytes(), TEST_KEY),
        Err(UpdateError::BadSignature)
    ));
}

#[test]
fn manifests_reject_ambiguous_fields_paths_and_encodings() {
    for text in [
        TEST_MANIFEST.replace('\n', "\r\n"),
        format!("\u{feff}{TEST_MANIFEST}"),
        format!("version=9.8.7\n{TEST_MANIFEST}"),
        format!("{TEST_MANIFEST}extra=x\n"),
        TEST_MANIFEST.replace("/pleiades-org/Core/", "/other/repo/"),
        TEST_MANIFEST.replace("core-v2.exe", "../core-v2.exe"),
        TEST_MANIFEST.replace("core-v2.exe", "core-v2.exe?other=1"),
        TEST_MANIFEST.replace("version=9.8.7", "version=09.8.7"),
        "x".repeat(MAX_MANIFEST_BYTES + 1),
    ] {
        assert!(Manifest::verify_with_key(text.as_bytes(), TEST_KEY).is_err());
    }
}

#[test]
fn versions_are_numeric_and_strict() {
    assert!("2.10.0".parse::<Version>().unwrap() > "2.9.9".parse().unwrap());
    for text in [
        "",
        "2.1",
        "2.1.0.1",
        "v2.1.0",
        "2.01.0",
        "2.1.0-beta",
        "2.1.-1",
        "4294967296.0.0",
    ] {
        assert!(text.parse::<Version>().is_err(), "{text}");
    }
}

#[test]
fn sha256_matches_the_published_abc_vector() {
    let digest: String = crypto::sha256(b"abc")
        .unwrap()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn disabled_services_never_start_network_work() {
    let service = UpdateService::new(false, UpdateMode::Automatic);
    service.refresh(HWND::default());
    assert!(!service.working());
    service.apply_staged().unwrap();
}

#[test]
fn update_checks_from_at_update_wait_a_minute_between_presses() {
    let now = Instant::now();
    assert!(manual_check_allowed(None, now));
    assert!(!manual_check_allowed(
        Some(now),
        now + Duration::from_secs(59)
    ));
    assert!(manual_check_allowed(Some(now), now + MANUAL_CHECK_COOLDOWN));
    // A clock read before the last press never counts as later.
    assert!(!manual_check_allowed(
        Some(now + Duration::from_secs(5)),
        now
    ));
}

#[test]
fn manual_checks_bypass_daily_and_failure_deadlines_in_enabled_modes() {
    let wall_clock_now = UNIX_EPOCH + CHECK_AFTER;
    let monotonic_now = Instant::now();
    for mode in [UpdateMode::Automatic, UpdateMode::NotifyOnly] {
        for backoff in [CHECK_AFTER, RETRY_AFTER] {
            let service = UpdateService::new(false, mode);
            let mut shared = service.shared.lock().unwrap();
            shared.next_check = wall_clock_now + backoff;
            shared.loaded_stage = true;

            let download_due =
                shared.begin_check(CheckTrigger::Manual, wall_clock_now, monotonic_now);

            assert_eq!(download_due, Some(true));
            assert!(shared.working);
            assert_eq!(shared.last_manual_check, Some(monotonic_now));
            assert_eq!(shared.next_check, wall_clock_now + RETRY_AFTER);
        }
    }
}

#[test]
fn manual_cooldown_blocks_expired_automatic_deadlines_until_sixty_seconds() {
    let wall_clock_now = UNIX_EPOCH + CHECK_AFTER;
    let monotonic_now = Instant::now();
    let service = UpdateService::new(false, UpdateMode::Automatic);
    let mut shared = service.shared.lock().unwrap();
    shared.last_manual_check = Some(monotonic_now);
    shared.next_check = UNIX_EPOCH;

    let too_soon = shared.begin_check(
        CheckTrigger::Manual,
        wall_clock_now,
        monotonic_now + Duration::from_secs(59),
    );

    assert_eq!(too_soon, None);
    assert!(!shared.working);
    assert!(!shared.loaded_stage);
    assert_eq!(shared.last_manual_check, Some(monotonic_now));
    assert_eq!(shared.next_check, UNIX_EPOCH);

    let after_cooldown = shared.begin_check(
        CheckTrigger::Manual,
        wall_clock_now,
        monotonic_now + MANUAL_CHECK_COOLDOWN,
    );

    assert_eq!(after_cooldown, Some(true));
    assert!(shared.working);
    assert_eq!(
        shared.last_manual_check,
        Some(monotonic_now + MANUAL_CHECK_COOLDOWN)
    );
}

#[test]
fn automatic_checks_keep_daily_and_failure_backoff_without_consuming_manual_cooldown() {
    let wall_clock_now = UNIX_EPOCH + CHECK_AFTER;
    let monotonic_now = Instant::now();
    for backoff in [CHECK_AFTER, RETRY_AFTER] {
        let service = UpdateService::new(false, UpdateMode::Automatic);
        let mut shared = service.shared.lock().unwrap();
        let deadline = wall_clock_now + backoff;
        shared.next_check = deadline;

        let cached_check =
            shared.begin_check(CheckTrigger::Automatic, wall_clock_now, monotonic_now);

        assert_eq!(cached_check, Some(false));
        assert_eq!(shared.next_check, deadline);
        assert_eq!(shared.last_manual_check, None);
        shared.working = false;
        assert_eq!(
            shared.begin_check(CheckTrigger::Automatic, wall_clock_now, monotonic_now),
            None
        );

        let due_check = shared.begin_check(CheckTrigger::Automatic, deadline, monotonic_now);

        assert_eq!(due_check, Some(true));
        assert_eq!(shared.next_check, deadline + RETRY_AFTER);
        assert_eq!(shared.last_manual_check, None);
    }
}

#[test]
fn in_flight_and_shutdown_checks_do_not_consume_manual_cooldown() {
    let wall_clock_now = UNIX_EPOCH + CHECK_AFTER;
    let monotonic_now = Instant::now();
    let service = UpdateService::new(false, UpdateMode::Automatic);
    let mut shared = service.shared.lock().unwrap();
    shared.working = true;

    assert_eq!(
        shared.begin_check(CheckTrigger::Manual, wall_clock_now, monotonic_now),
        None
    );
    assert_eq!(shared.last_manual_check, None);
    shared.working = false;
    shared.shutdown = true;
    assert_eq!(
        shared.begin_check(CheckTrigger::Manual, wall_clock_now, monotonic_now),
        None
    );
    assert_eq!(shared.last_manual_check, None);
    assert!(!shared.working);
    assert_eq!(shared.next_check, UNIX_EPOCH);
}

#[test]
fn checking_now_still_respects_off_disabled_and_ready_updates() {
    let disabled = UpdateService::new(false, UpdateMode::Automatic);
    disabled.check_now(HWND::default());
    assert!(!disabled.working());
    assert_eq!(disabled.shared.lock().unwrap().last_manual_check, None);
    let mut service = UpdateService::new(false, UpdateMode::Off);
    service.enabled = true;
    service.check_now(HWND::default());
    assert!(!service.working());
    // A downloaded update is installed with @update, not checked for again.
    service.set_mode(UpdateMode::Automatic);
    service.shared.lock().unwrap().state = UpdateState::Staged("9.8.7".parse().unwrap());
    service.check_now(HWND::default());
    assert!(!service.working());
    // Refused presses do not start the minute's wait.
    assert_eq!(service.shared.lock().unwrap().last_manual_check, None);
}

#[test]
fn off_and_retry_deadlines_suppress_network_and_mode_changes_preserve_backoff() {
    let mut service = UpdateService::new(false, UpdateMode::Off);
    service.enabled = true;
    service.refresh(HWND::default());
    assert!(!service.working());
    let deadline = SystemTime::now() + CHECK_AFTER;
    service.shared.lock().unwrap().next_check = deadline;
    service.set_mode(UpdateMode::Automatic);
    service.shared.lock().unwrap().loaded_stage = true;
    service.refresh(HWND::default());
    assert!(!service.working());
    assert_eq!(service.shared.lock().unwrap().next_check, deadline);
    service.set_mode(UpdateMode::NotifyOnly);
    service.shared.lock().unwrap().state = UpdateState::Staged("9.8.7".parse().unwrap());
    service.set_mode(UpdateMode::Off);
    assert!(matches!(service.state(), UpdateState::Available(_)));
    service.refresh(HWND::default());
    assert!(!service.working());
}

#[test]
fn notify_and_off_leave_existing_stages_untouched_on_exit() {
    let folder = std::env::temp_dir().join(format!("core-update-exit-{}", std::process::id()));
    fs::create_dir_all(&folder).unwrap();
    let installation = Installation::new(folder.join("core-v2.exe"));
    fs::write(&installation.executable, b"original").unwrap();
    fs::write(&installation.staged, b"pending update").unwrap();
    for mode in [UpdateMode::NotifyOnly, UpdateMode::Off] {
        let mut service = UpdateService::new(false, mode);
        service.enabled = true;
        service.installation = Some(installation.clone());
        service.shared.lock().unwrap().state = UpdateState::Staged("9.8.7".parse().unwrap());

        service.apply_staged().unwrap();

        assert_eq!(fs::read(&installation.executable).unwrap(), b"original");
        assert_eq!(fs::read(&installation.staged).unwrap(), b"pending update");
        assert!(!installation.previous.exists());
        assert!(!installation.pending.exists());
        assert!(!installation.helper.exists());
    }
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_queued_check_observes_off_before_fetching_a_manifest() {
    let service = UpdateService::new(false, UpdateMode::Off);
    let installation = Installation::new(
        std::env::temp_dir().join(format!("core-off-{}.exe", std::process::id())),
    );
    assert!(matches!(
        check_release(&installation, &service.shared, true, None).unwrap(),
        UpdateState::UpToDate
    ));
}
