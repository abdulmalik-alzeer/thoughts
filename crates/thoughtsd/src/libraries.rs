//! Several libraries (FEAT-32): a work library and a personal one, one per client or employer.
//!
//! The list lives in `Settings::libraries`; the open library's own settings are the top-level
//! fields of `Settings` (everything reads them there), each other library's wait in its entry
//! (`settings::LibrarySettings`). Chats carry the library they were started in (`Chat::library`,
//! see `wraps::Scope`), trust and history are kept per folder already.

#[cfg(test)]
mod tests {
    use crate::chats::{Chat, Msg};
    use crate::wraps::{self, testutil as t};

    #[tokio::test]
    async fn chats_from_before_libraries_belong_to_the_one_open_at_the_upgrade_and_stay_there() {
        let (st, a) = t::library().await;
        let b = a.parent().unwrap().join("second");
        std::fs::create_dir_all(&b).unwrap();
        // Chats written by 0.0.8: no library in the file at all.
        let old: Chat = serde_json::from_str(r#"{"id":"0a8-chat","title":"Old chat","kind":"chat","messages":[{"id":"m1","role":"user","text":"hello","ts":"2026-05-01T10:00:00Z"}]}"#).unwrap();
        assert!(old.library.is_none());
        st.chats.insert(old).await;
        let custom = Chat { kind: "customize".into(), ..Default::default() };
        let custom_id = custom.id.clone();
        st.chats.insert(custom).await;
        // The first start of this version: the chats are the open library's.
        wraps::recover_chats(&st).await;
        let key_a = a.to_string_lossy().to_string();
        assert_eq!(st.chats.meta("0a8-chat").await.unwrap().library.as_deref(), Some(key_a.as_str()));
        assert!(st.chats.meta(&custom_id).await.unwrap().library.is_none(), "customising the app belongs to no library");
        // Another library is opened and the app starts again: they stay where they were tied once.
        st.settings.write().await.open_folder(&b.to_string_lossy(), None);
        wraps::recover_chats(&st).await;
        assert_eq!(st.chats.meta("0a8-chat").await.unwrap().library.as_deref(), Some(key_a.as_str()));
        assert!(!wraps::Scope::now(&st).await.holds(Some(&key_a)), "not listed while the second library is open");
        // A chat started now belongs to the second library, and an older one never moves into it.
        let mut new = Chat::default();
        new.messages.push(Msg::new("user", "about the second library"));
        new.library = Some(b.to_string_lossy().to_string());
        st.chats.insert(new).await;
        let scope = wraps::Scope::now(&st).await;
        let here: Vec<String> = st.chats.list().await.into_iter().filter(|c| c.kind == "chat" && scope.holds(c.library.as_deref())).map(|c| c.title).collect();
        assert_eq!(here, ["New chat"]);
        std::fs::remove_dir_all(a.parent().unwrap()).unwrap();
    }
}
