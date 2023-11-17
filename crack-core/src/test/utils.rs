#[cfg(test)]
mod test {
    use crate::utils::build_log_embed;
    use tokio;

    #[tokio::test]
    async fn test_build_log_embed() {
        let embed = build_log_embed("1111", "test", "test", "test")
            .await
            .unwrap();
        println!("{:?}", embed);
        // FIXME: How can be actually access the CreateEmbed and truly test this?
        assert!(true);
    }
}
