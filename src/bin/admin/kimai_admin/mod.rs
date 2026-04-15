// src/bin/admin/kimai_admin/mod.rs
pub mod bootstrap;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

// ── Response types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Customer {
    pub id: u32,
    pub name: String,
    pub currency: String,
    pub timezone: String,
    pub visible: bool,
    pub color: Option<String>,
    pub email: Option<String>,
    pub budget: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Project {
    pub id: u32,
    pub name: String,
    pub customer: u32,
    pub visible: bool,
    pub color: Option<String>,
    pub budget: Option<f64>,
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Activity {
    pub id: u32,
    pub name: String,
    pub project: Option<u32>,
    pub visible: bool,
    pub color: Option<String>,
    pub budget: Option<f64>,
}

// ── Request types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateCustomerRequest {
    pub name: String,
    pub country: String,
    pub currency: String,
    pub timezone: String,
    pub visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub customer: u32,
    pub visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateActivityRequest {
    pub name: String,
    pub visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_type: Option<String>,
}

// ── Client ────────────────────────────────────────────────────────────────

pub struct KimaiAdminClient {
    http: Client,
    base_url: String,
    token: String,
}

impl KimaiAdminClient {
    pub fn new(base_url: &str, token: &str) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.bearer_auth(&self.token)
    }

    async fn check(&self, resp: reqwest::Response, context: &str) -> Result<reqwest::Response> {
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("{context}: HTTP {status} — {body}");
    }

    // ── Customers ────────────────────────────────────────────────────────

    pub async fn list_customers(&self) -> Result<Vec<Customer>> {
        let url = format!("{}/api/customers?visible=1&order=ASC&orderBy=name", self.base_url);
        let resp = self.auth(self.http.get(&url)).send().await
            .context("list_customers: request failed")?;
        Ok(self.check(resp, "list_customers").await?.json().await?)
    }

    pub async fn get_customer(&self, id: u32) -> Result<Customer> {
        let url = format!("{}/api/customers/{id}", self.base_url);
        let resp = self.auth(self.http.get(&url)).send().await
            .context("get_customer: request failed")?;
        Ok(self.check(resp, "get_customer").await?.json().await?)
    }

    pub async fn create_customer(&self, req: CreateCustomerRequest) -> Result<Customer> {
        let url = format!("{}/api/customers", self.base_url);
        let resp = self.auth(self.http.post(&url)).json(&req).send().await
            .context("create_customer: request failed")?;
        Ok(self.check(resp, "create_customer").await?.json().await?)
    }

    pub async fn delete_customer(&self, id: u32) -> Result<()> {
        let url = format!("{}/api/customers/{id}", self.base_url);
        let resp = self.auth(self.http.delete(&url)).send().await
            .context("delete_customer: request failed")?;
        self.check(resp, "delete_customer").await?;
        Ok(())
    }

    // ── Projects ─────────────────────────────────────────────────────────

    pub async fn list_projects(&self, customer: Option<u32>) -> Result<Vec<Project>> {
        let mut url = format!("{}/api/projects?visible=1&order=ASC&orderBy=name", self.base_url);
        if let Some(c) = customer {
            url.push_str(&format!("&customer={c}"));
        }
        let resp = self.auth(self.http.get(&url)).send().await
            .context("list_projects: request failed")?;
        Ok(self.check(resp, "list_projects").await?.json().await?)
    }

    pub async fn get_project(&self, id: u32) -> Result<Project> {
        let url = format!("{}/api/projects/{id}", self.base_url);
        let resp = self.auth(self.http.get(&url)).send().await
            .context("get_project: request failed")?;
        Ok(self.check(resp, "get_project").await?.json().await?)
    }

    pub async fn create_project(&self, req: CreateProjectRequest) -> Result<Project> {
        let url = format!("{}/api/projects", self.base_url);
        let resp = self.auth(self.http.post(&url)).json(&req).send().await
            .context("create_project: request failed")?;
        Ok(self.check(resp, "create_project").await?.json().await?)
    }

    pub async fn delete_project(&self, id: u32) -> Result<()> {
        let url = format!("{}/api/projects/{id}", self.base_url);
        let resp = self.auth(self.http.delete(&url)).send().await
            .context("delete_project: request failed")?;
        self.check(resp, "delete_project").await?;
        Ok(())
    }

    // ── Activities ────────────────────────────────────────────────────────

    pub async fn list_activities(&self, project: Option<u32>) -> Result<Vec<Activity>> {
        let mut url = format!("{}/api/activities?visible=1&order=ASC&orderBy=name", self.base_url);
        if let Some(p) = project {
            url.push_str(&format!("&project={p}"));
        }
        let resp = self.auth(self.http.get(&url)).send().await
            .context("list_activities: request failed")?;
        Ok(self.check(resp, "list_activities").await?.json().await?)
    }

    pub async fn get_activity(&self, id: u32) -> Result<Activity> {
        let url = format!("{}/api/activities/{id}", self.base_url);
        let resp = self.auth(self.http.get(&url)).send().await
            .context("get_activity: request failed")?;
        Ok(self.check(resp, "get_activity").await?.json().await?)
    }

    pub async fn create_activity(&self, req: CreateActivityRequest) -> Result<Activity> {
        let url = format!("{}/api/activities", self.base_url);
        let resp = self.auth(self.http.post(&url)).json(&req).send().await
            .context("create_activity: request failed")?;
        Ok(self.check(resp, "create_activity").await?.json().await?)
    }

    pub async fn delete_activity(&self, id: u32) -> Result<()> {
        let url = format!("{}/api/activities/{id}", self.base_url);
        let resp = self.auth(self.http.delete(&url)).send().await
            .context("delete_activity: request failed")?;
        self.check(resp, "delete_activity").await?;
        Ok(())
    }
}
