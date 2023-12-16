use std::fmt::format;

use reqwest::{self, RequestBuilder};

struct Header {
    product: String,
    version: f32,
    content_type: String,
}

impl Header {
    fn new() -> RequestBuilder {
        Self {
            product: format!("tasty-options-trader"),
            version: 0.1,
            content_type: format!("application/json"),
        }
    }
}

struct HttpClient {}

impl HttpClient {
    fn build_request(request: RequestBuilder) -> RequestBuilder {
        request.header("product", format!("tasty-options-trader"));
        request.header("version", 0.1);
        request.header("product", format!("application/json"));
        request
    }

    async fn get<Payload>(url: &str) -> Result<Payload, reqwest::Error> {
        // Create a reqwest Client
        let client = reqwest::Client::new();

        // Send a GET request to the specified URL
        let response = Self::build_request(client.get(url)).send().await?;

        // Check if the request was successful (HTTP status code 2xx)
        if response.status().is_success() {
            // Print the response body as a String
            let body = response.text().await?;
            println!("Response body: {}", body);
        } else {
            // Print the HTTP status code and reason phrase
            println!("Request failed with status: {}", response.status());
        }

        Ok(())
    }

    async fn post_request<Payload>(url: &str, data: Payload) -> Result<(), reqwest::Error> {
        let client = reqwest::Client::new();
        let response = Self::build_request(client.post(url))
            .json(data)
            .send()
            .await?;

        if response.status().is_success() {
            let body = response.text().await?;
            println!("POST Response body: {}", body);
        } else {
            println!("POST Request failed with status: {}", response.status());
        }

        Ok(())
    }

    async fn update_request(url: &str, data: &Data) -> Result<(), reqwest::Error> {
        let client = reqwest::Client::new();
        let response = Self::build_request(client.put(url))
            .json(data)
            .send()
            .await?;

        if response.status().is_success() {
            let body = response.text().await?;
            println!("PUT Response body: {}", body);
        } else {
            println!("PUT Request failed with status: {}", response.status());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_successful_requests() {
        // Specify the URL you want to use for testing
        let url = "https://www.example.com";

        // Test a successful GET request
        assert!(get_request(url).await.is_ok());

        // Test a successful POST request
        let data = Data {
            key: "example_key".to_string(),
            value: "example_value".to_string(),
        };
        assert!(post_request(url, &data).await.is_ok());

        // Test a successful PUT request
        let updated_data = Data {
            key: "updated_key".to_string(),
            value: "updated_value".to_string(),
        };
        assert!(update_request(url, &updated_data).await.is_ok());
    }

    #[tokio::test]
    async fn test_failed_requests() {
        // Specify a non-existent URL for testing failed requests
        let url = "https://www.nonexistent-url.com";

        // Test a failed GET request
        assert!(get_request(url).await.is_err());

        // Test a failed POST request
        let data = Data {
            key: "example_key".to_string(),
            value: "example_value".to_string(),
        };
        assert!(post_request(url, &data).await.is_err());

        // Test a failed PUT request
        let updated_data = Data {
            key: "updated_key".to_string(),
            value: "updated_value".to_string(),
        };
        assert!(update_request(url, &updated_data).await.is_err());
    }
    #[tokio::test]
    async fn test_auth_with_broker() {}
    #[tokio::test]
    async fn test_() {}
    #[tokio::test]
    async fn test_failed_requests() {}
    #[tokio::test]
    async fn test_failed_requests() {}
    #[tokio::test]
    async fn test_failed_requests() {}
}
