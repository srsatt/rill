#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> AuthService {
        AuthService::new(DbPool::open_in_memory().unwrap(), 30, 180, 10, 3)
    }

    fn user(service: &AuthService) -> User {
        service
            .create_user(
                "alice",
                Some("Alice@example.com"),
                "correct horse battery",
                Role::Admin,
            )
            .unwrap()
    }

    #[test]
    fn stores_only_session_hash_and_authorizes_role() {
        let service = service();
        user(&service);
        let session = service
            .authenticate_at(
                "alice@example.com",
                "correct horse battery",
                None,
                None,
                1_000,
            )
            .unwrap();
        let connection = service.pool.connection().unwrap();
        let stored: Vec<u8> = connection
            .query_row("SELECT token_hash FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored.len(), 32);
        assert_ne!(stored, session.token.expose().as_bytes());
        drop(connection);
        let principal = service
            .principal(session.token.expose(), SessionKind::Browser, 1_001)
            .unwrap();
        assert_eq!(principal.user.role, Role::Admin);
        principal.require_admin().unwrap();
    }

    #[test]
    fn pairing_is_single_use_and_token_is_not_stored() {
        let service = service();
        let user = user(&service);
        let pairing = service
            .create_pairing_code_at(&user.id, "Kobo", 2_000)
            .unwrap();
        let reader = service
            .consume_pairing_code_at(pairing.code.expose(), "client-a", None, None, 2_001)
            .unwrap();
        let connection = service.pool.connection().unwrap();
        let stored: Vec<u8> = connection
            .query_row("SELECT token_hash FROM device_sessions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_ne!(stored, reader.token.expose().as_bytes());
        drop(connection);
        assert!(matches!(
            service.consume_pairing_code_at(pairing.code.expose(), "client-b", None, None, 2_002),
            Err(AuthError::PairingReplay)
        ));
    }

    #[test]
    fn expired_pairing_code_is_rejected() {
        let service = service();
        let user = user(&service);
        let pairing = service
            .create_pairing_code_at(&user.id, "Reader", 1_000)
            .unwrap();
        assert!(matches!(
            service.consume_pairing_code_at(pairing.code.expose(), "client-a", None, None, 1_601),
            Err(AuthError::PairingExpired)
        ));
    }

    #[test]
    fn failed_pairing_attempts_are_persistently_limited() {
        let service = service();
        for time in 0..3 {
            assert!(matches!(
                service.consume_pairing_code_at("AAAA-AAAA", "client-a", None, None, 2_000 + time),
                Err(AuthError::InvalidPairingCode)
            ));
        }
        assert!(matches!(
            service.consume_pairing_code_at("AAAA-AAAA", "client-a", None, None, 2_004),
            Err(AuthError::RateLimited)
        ));
    }

    #[test]
    fn reader_cannot_pass_admin_authorization() {
        let principal = Principal {
            user: User {
                id: "u".into(),
                username: "reader".into(),
                email: None,
                role: Role::Admin,
                disabled: false,
            },
            session_id: "s".into(),
            kind: SessionKind::Reader,
        };
        assert!(matches!(
            principal.require_admin(),
            Err(AuthError::Forbidden)
        ));
    }
}
