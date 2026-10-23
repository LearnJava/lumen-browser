use super::*;

#[test]
fn build_origin_standard_ports_omitted() {
    let http = Url::parse("http://example.com/path").unwrap();
    assert_eq!(build_origin(&http), "http://example.com");

    let https = Url::parse("https://example.com/path").unwrap();
    assert_eq!(build_origin(&https), "https://example.com");
}

#[test]
fn build_origin_non_standard_port_included() {
    let url = Url::parse("https://example.com:8443/path").unwrap();
    assert_eq!(build_origin(&url), "https://example.com:8443");

    let url2 = Url::parse("http://localhost:3000/api").unwrap();
    assert_eq!(build_origin(&url2), "http://localhost:3000");
}

#[test]
fn in_memory_interceptor_miss_returns_none() {
    let i = InMemoryFetchInterceptor::new();
    let url = Url::parse("https://example.com/page").unwrap();
    assert!(i.intercept(&url, "https://example.com").is_none());
}

#[test]
fn in_memory_interceptor_hit_returns_body() {
    let i = InMemoryFetchInterceptor::new();
    i.insert(
        "https://example.com",
        "https://example.com/page",
        b"hello".to_vec(),
    );
    let url = Url::parse("https://example.com/page").unwrap();
    assert_eq!(
        i.intercept(&url, "https://example.com"),
        Some(b"hello".to_vec())
    );
}

#[test]
fn in_memory_interceptor_wrong_origin_miss() {
    let i = InMemoryFetchInterceptor::new();
    i.insert("https://other.com", "https://example.com/page", b"x".to_vec());
    let url = Url::parse("https://example.com/page").unwrap();
    assert!(i.intercept(&url, "https://example.com").is_none());
}

#[test]
fn network_transport_uses_interceptor_before_network() {
    // Interceptor возвращает тело — сетевого запроса не должно быть.
    // Проверяем на несуществующем хосте: если interceptor не сработает,
    // fetch провалится с network error; если сработает — Ok.
    let interceptor = Arc::new(InMemoryFetchInterceptor::new());
    interceptor.insert(
        "https://no-such-host-lumen-test.invalid",
        "https://no-such-host-lumen-test.invalid/data",
        b"intercepted".to_vec(),
    );
    let client = HttpClient::new().with_interceptor(interceptor);
    let url = Url::parse("https://no-such-host-lumen-test.invalid/data").unwrap();
    let result = client.fetch(&url).unwrap();
    assert_eq!(result, b"intercepted");
}
