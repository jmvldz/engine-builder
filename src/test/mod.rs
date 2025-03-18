mod mock_llm;

pub async fn init_mocks() {
    mock_llm::init_mock_client().await;
}
