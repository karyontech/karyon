pub async fn sleep(duration: std::time::Duration) {
    #[cfg(feature = "smol")]
    smol::Timer::after(duration).await;
    #[cfg(feature = "tokio")]
    tokio::time::sleep(duration).await;
}
