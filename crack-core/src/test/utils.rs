#[cfg(test)]
mod test {
    use crate::messaging::interface::build_log_embed;

    #[tokio::test]
    async fn test_build_log_embed() {
        let embed = build_log_embed("test", "test", "test").await.unwrap();
        println!("{embed:?}");
        // FIXME: How can be actually access the CreateEmbed and truly test this?
    }
}
