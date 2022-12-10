use std::{str::FromStr};

use reqwest::RequestBuilder;
use tracing::{info, warn};

use crate::docker_client::{www_authenticate::AuthenticationChallenge, authentication_strategies::{AnonymousAuthStrategy, HttpBasicAuthStrategy, BearerTokenAuthStrategy}};

use super::{www_authenticate::WwwAuthenticateError, authentication_strategies::AuthenticationStrategy};

#[derive(thiserror::Error, Debug)]
pub enum DockerClientError {
    #[error("Unexpected status code {0}")]
    UnexpectedStatusCode(u16),

    #[error("Provided credentials are errorneous or unable to be provided when requested")]
    BadAuthenticationCredentials,

    #[error(transparent)]
    WwwAuthenticateParseError(#[from] WwwAuthenticateError),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error)
}

pub struct DockerClient {
    auth_strat: Option<Box<dyn AuthenticationStrategy>>,
    registry: String,
    container: String,
    http_client: reqwest::Client
}

impl DockerClient {
    pub fn new(registry: &str, container: &str) -> Self {
        let client = reqwest::Client::new();

        Self {
            auth_strat: None,
            registry: registry.to_string(),
            container: container.to_string(),
            http_client: client,
        }
    }

    pub async fn authenticate(&mut self, registry_username: Option<&str>, registry_password: Option<&str>) -> Result<(), DockerClientError> {
        if self.auth_strat.is_some() {
            return Ok(());
        }

        // Fetch the base and see what the authorization header has to say
        info!("Discovering authentication strategies for the registry {}", self.registry);

        let url = url::Url::from_str(&format!("https://{}/v2/", self.registry)).unwrap();
        let base_response = self.http_client.get(url).send().await.unwrap();

        // If the server responds 200 immediately, we'll consider we don't need authentication.
        if base_response.status() == 200 {
            info!("Got 200, assuming repository can be accessed without any credentials");
            self.auth_strat = Some(Box::new(AnonymousAuthStrategy));
            return Ok(());
        }

        // The next thing we probably will have a 401 Unauthorized code with a WWW-Authenticate header.
        // We don't care about the rest.
        if base_response.status() != 401 {
            warn!("Got a response with status {}, expected 401", base_response.status());
            return Err(DockerClientError::UnexpectedStatusCode(base_response.status().as_u16()));
        }

        // This will be a crude parser. It DOES NOT support registries with multiple challenges and WILL be thrown off
        // if a registry sends multiple challenges.
        let www_authenticate = base_response.headers()
            .get("WWW-Authenticate")
            .expect("If we received a 401, we should have a WWW-Authenticate header. What's the point otherwise ?")
            .to_str()
            .expect("The header should contain only UTF8 characters");
        info!("Got authentication challenge header [{}]", www_authenticate);

        let auth_challenge = AuthenticationChallenge::from_www_authenticate(&www_authenticate)?;

        let mut auth_strategy: Box<dyn AuthenticationStrategy> = match auth_challenge {
            AuthenticationChallenge::Basic(_) if registry_username.is_some() => {
                info!("Applying HTTP Basic for registry {}", self.registry);
                Box::new(HttpBasicAuthStrategy::new(registry_username.unwrap(), registry_password))
            },

            AuthenticationChallenge::Basic(_) => {
                warn!("No provided credential for auth method Basic");
                return Err(DockerClientError::BadAuthenticationCredentials);
            }

            AuthenticationChallenge::Bearer(_) => {
                info!("Applying Bearer token authentication for registry {}", self.registry);
                Box::new(BearerTokenAuthStrategy::new(&self.container))
            }
        };

        auth_strategy.execute_authentication(
            &self.http_client, auth_challenge.authentication_parameters(), 
            registry_username, 
            registry_password
        ).await?;

        self.auth_strat = Some(auth_strategy);

        if let Err(auth_error) = self.check_authentication().await {
            self.auth_strat = None;
            return Err(auth_error);
        }

        Ok(())
    }

    pub async fn query_base(&self) -> Result<(), DockerClientError> {
        let query = self.http_client.get(format!("https://{}/v2/", self.registry));
        let query = self.add_authentication(query);
        let response = query.send().await?;

        if response.status() != 200 {
            return Err(DockerClientError::UnexpectedStatusCode(response.status().as_u16()));
        }

        Ok(())
    }

    fn add_authentication(&self, request: RequestBuilder) -> RequestBuilder {
        self.auth_strat.as_ref().unwrap().inject_authentication(request)
    }

    async fn check_authentication(&self) -> Result<(), DockerClientError>{
        let response = self.query_base().await;

        match response {
            Err(DockerClientError::UnexpectedStatusCode(code)) if code == 401 => {
                warn!("Invalid credentials");
                return Err(DockerClientError::BadAuthenticationCredentials);
            },

            Err(other_error) => {
                warn!("Other client error: {:?}", other_error);
                return Err(other_error);
            },

            Ok(_) => {
                info!("Provided credentials are OK");
                return Ok(())
            },
        }
    }
}