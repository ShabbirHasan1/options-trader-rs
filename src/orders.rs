use std::{rc::Rc, sync::Arc};

use crate::web_client::WebClient;

pub struct Orders {
    web_client: Arc<WebClient>,
}

impl Orders {
    pub fn new(web_client: Arc<WebClient>) -> Self {
        Self { web_client }
    }
}
