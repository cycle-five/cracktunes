#[cfg(test)]
mod tests {
    use crate::http_utils::resolve_final_url;

    #[tokio::test]
    async fn test_resolve_final_url() {
        let original = "https://spotify.link/fLKHSV0gZDb";
        let final_want = "https://open.spotify.com/track/70C4NyhjD5OZUMzvWZ3njJ?si=4P3mzfpRQ4WisJuUCQ_k9w&utm_source=copy-link&utm_medium=copy-link&nd=1&_branch_match_id=link-1243034348111107939";

        let final_got = resolve_final_url(original).await.unwrap();

        assert_eq!(final_got, final_want);
    }
}
