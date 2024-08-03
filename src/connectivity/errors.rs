enum ApiError {
    InvalidRequest,
    AuthorizationError,
    Forbidden,
    NotFound,
    TooManyRequests,
    ServerError(String), // Assuming the server error includes an identifier
}

impl ApiError {
    fn to_code(&self) -> u16 {
        match self {
            ApiError::InvalidRequest => 400,
            ApiError::AuthorizationError => 401,
            ApiError::Forbidden => 403,
            ApiError::NotFound => 404,
            ApiError::TooManyRequests => 429,
            ApiError::ServerError(_) => 500,
        }
    }

    fn from_code(code: u16, message: Option<String>) -> Option<ApiError> {
        match code {
            400 => Some(ApiError::InvalidRequest),
            401 => Some(ApiError::AuthorizationError),
            403 => Some(ApiError::Forbidden),
            404 => Some(ApiError::NotFound),
            429 => Some(ApiError::TooManyRequests),
            500 => Some(ApiError::ServerError(
                message.unwrap_or_else(|| "Unknown Error".to_string()),
            )),
            _ => None, // For unrecognized codes
        }
    }

    fn description(&self) -> String {
        match self {
            ApiError::InvalidRequest => "Invalid request. Often indicates that your request body has missing or invalid parameters.".to_owned(),
            ApiError::AuthorizationError => "Authorization token has expired or is invalid. Also indicates an invalid username/password when logging in.".to_owned(),
            ApiError::Forbidden => "User is not authorized to access this resource. This may occur when a customer tries to access data for an account belonging to a different customer, for example.".to_owned(),
            ApiError::NotFound => "Endpoint or resource not found. This may occur when attempting to fetch data that does not exist (a specific order, for example).".to_owned(),
            ApiError::TooManyRequests => "Too Many Requests. This occurs when you send a high amount of requests in a short period of time to the point where it exceeds reasonable thresholds.".to_owned(),
            ApiError::ServerError(identifier) => format!("Indicates an issue with tastytrade's servers. Returns a support identifier ({}) that our team can use to track down the issue.", identifier).to_owned(),
        }
    }
}
