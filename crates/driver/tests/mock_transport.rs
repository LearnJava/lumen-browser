//! Интеграционный тест: использование MockTransport с InProcessSession.

use lumen_core::ext::NetworkTransport;
use lumen_core::url::Url;
use lumen_driver::{BrowserSession, InProcessSession};
use lumen_network::MockTransport;

#[test]
fn test_mock_transport_returns_fixture() {
    let mut transport = MockTransport::new();
    let html = b"<html><head><title>Test</title></head><body><p>Hello</p></body></html>";
    let url = "http://example.com/test.html";

    // Зарегистрировать fixture для URL
    transport.add_fixture(url, html.to_vec());

    // Проверить, что fetch() возвращает зарегистрированные данные
    let parsed_url = Url::parse(url).unwrap();
    let result = transport.fetch(&parsed_url).unwrap();
    assert_eq!(result, html);
}

#[test]
fn test_mock_transport_missing_fixture() {
    let transport = MockTransport::new();
    let url = "http://example.com/missing.html";

    let parsed_url = Url::parse(url).unwrap();
    let result = transport.fetch(&parsed_url);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("No fixture registered"));
}

#[test]
fn test_mock_transport_multiple_urls() {
    let mut transport = MockTransport::new();

    let url1 = "http://example.com/page1.html";
    let data1 = b"<html><body>Page 1</body></html>";
    transport.add_fixture(url1, data1.to_vec());

    let url2 = "http://example.com/page2.html";
    let data2 = b"<html><body>Page 2</body></html>";
    transport.add_fixture(url2, data2.to_vec());

    // Проверить, что можно получить оба fixture'а
    let parsed_url1 = Url::parse(url1).unwrap();
    assert_eq!(transport.fetch(&parsed_url1).unwrap(), data1);

    let parsed_url2 = Url::parse(url2).unwrap();
    assert_eq!(transport.fetch(&parsed_url2).unwrap(), data2);
}

#[test]
fn test_inprocess_session_with_fixture_via_run_pipeline() {
    // Демонстрация: как использовать MockTransport-fixture с InProcessSession.
    //
    // Текущий API session.navigate() не поддерживает custom transport,
    // но можно использовать run_pipeline() напрямую с fixture-данными,
    // или использовать file:// URLs для тестов.

    let mut session = InProcessSession::new();
    let html = b"<html><body><h1>Title</h1></body></html>";

    // В текущей реализации можем тестировать через обработку HTML
    // напрямую (через file:// или прямую передачу в run_pipeline).
    // Полная интеграция с custom transport — в следующих задачах.

    // Для теста используем file:// URL
    let temp_file = "test-fixture.html";
    std::fs::write(temp_file, html).unwrap();

    let result = session.navigate(&format!("file://{}", temp_file));
    assert!(result.is_ok());

    let url = session.current_url();
    assert!(url.contains(temp_file));

    // Очистка
    let _ = std::fs::remove_file(temp_file);
}
