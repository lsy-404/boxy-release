use super::*;

#[test]
fn setup_shows_the_initial_session_path_and_local_selection_controls() {
    let path = Path::new(r"C:\Users\TestUser\Dreamtonics\session");
    let html = render_html("https://service.example.test", path);

    assert!(html.contains(r#"id="session-path""#));
    assert!(html.contains(r#"value="C:\Users\TestUser\Dreamtonics\session""#));
    let session_input = html
        .split("id=\"session-path\"")
        .nth(1)
        .unwrap()
        .split('>')
        .next()
        .unwrap();
    assert!(!session_input.contains("readonly"));
    assert!(html.contains(r#"id="choose-session""#));
    assert!(html.contains(r#"id="reset-session""#));
    assert!(html.contains("chooseSessionPath"));
    assert!(html.contains("resetSessionPath"));
    assert!(
        include_str!("../connector/ui/app.js").contains("sessionPath: sessionPath.value.trim()")
    );
}

#[test]
fn custom_initial_session_path_resets_to_the_distinct_default_path() {
    let initial = Path::new("C:\\custom\\session");
    let default = Path::new("C:\\default\\license\\session");
    let mut selection = SessionPathSelection::new(initial, default);

    assert_eq!(selection.selected, initial);
    selection.choose(Path::new("C:\\picked\\session").to_path_buf());
    assert_eq!(selection.selected, Path::new("C:\\picked\\session"));
    selection.reset();
    assert_eq!(selection.selected, default);
}

#[test]
fn setup_escapes_a_session_path_before_inserting_it_into_html() {
    let path = Path::new("C:\\data\\session\"><script>alert(1)</script>");
    let html = render_html("", path);

    assert!(html.contains("&quot;&gt;&lt;script&gt;"));
    assert!(!html.contains("<script>alert(1)</script>"));
}

#[test]
fn connect_rejects_empty_missing_and_directory_session_paths() {
    assert!(validate_session_path("").is_err());
    assert!(validate_session_path("   ").is_err());
    assert!(validate_session_path("C:\\missing\\session").is_err());

    let directory =
        std::env::temp_dir().join(format!("boxy-session-path-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let directory_path = directory.to_string_lossy();
    assert!(validate_session_path(&directory_path).is_err());

    let session = directory.join("session");
    std::fs::write(&session, [1; 8]).unwrap();
    let session_path = session.to_string_lossy();
    assert_eq!(validate_session_path(&session_path).unwrap(), session);
    assert_eq!(
        validate_session_path(&format!("  {session_path}  ")).unwrap(),
        session
    );
    let _ = std::fs::remove_dir_all(directory);
}
