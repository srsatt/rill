#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_matching_does_not_accept_host_prefixes() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://rill.example.evil"),
        );
        assert!(!valid_origin(&headers, &["https://rill.example".into()]));
    }

    #[test]
    fn origin_matching_accepts_configured_aliases() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://rill.home.example"),
        );
        assert!(valid_origin(
            &headers,
            &[
                "https://rill.example".into(),
                "https://rill.home.example".into(),
            ],
        ));
    }

    #[test]
    fn cookie_parser_matches_exact_name() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=x; rill_session=secret"),
        );
        assert_eq!(cookie(&headers, "rill_session").as_deref(), Some("secret"));
    }

    #[test]
    fn ip_summaries_are_coarse() {
        assert_eq!(ip_summary("192.0.2.44".parse().unwrap()), "192.0.2.0/24");
        assert_eq!(
            ip_summary("2001:db8:1234:5678::1".parse().unwrap()),
            "2001:db8:1234:5678::/64"
        );
    }

    #[test]
    fn production_session_cookies_follow_host_prefix_contract() {
        let browser = cookie_header(
            browser_session_cookie(true),
            "opaque",
            true,
            false,
            true,
            60,
        );
        let reader = cookie_header(reader_session_cookie(true), "opaque", true, true, true, 60);
        assert!(browser.starts_with("__Host-rill_session="));
        assert!(reader.starts_with("__Host-rill_reader="));
        for cookie in [browser, reader] {
            assert!(cookie.contains("; Secure"));
            assert!(cookie.contains("; HttpOnly"));
            assert!(cookie.contains("; Path=/"));
            assert!(!cookie.contains("Domain="));
        }
    }

    #[test]
    fn hydration_json_cannot_end_its_script_element() {
        let escaped = escape_json_for_html("{\"value\":\"</script>\u{2028}\u{2029}\"}");

        assert_eq!(
            escaped,
            "{\"value\":\"\\u003c/script>\\u2028\\u2029\"}"
        );
        assert!(!escaped.contains('<'));
        assert!(!escaped.contains('\u{2028}'));
        assert!(!escaped.contains('\u{2029}'));
    }

    #[test]
    fn quick_add_recognizes_telegram_channel_links_and_handles() {
        assert_eq!(
            telegram_username_from_input("https://t.me/cortex_pulse/123").as_deref(),
            Some("cortex_pulse")
        );
        assert_eq!(
            telegram_username_from_input("@cortex_pulse").as_deref(),
            Some("cortex_pulse")
        );
        assert_eq!(telegram_username_from_input("https://example.com"), None);
    }
}
