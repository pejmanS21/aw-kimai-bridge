// src/bin/admin/kimai_admin/bootstrap.rs

use super::{
    Activity, Customer,
    KimaiAdminClient, Project,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

// ── Blueprint TOML schema ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Blueprint {
    pub customers: Vec<BlueprintCustomer>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlueprintCustomer {
    pub name: String,
    #[serde(default = "default_country")]
    pub country: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<f64>,
    #[serde(default)]
    pub projects: Vec<BlueprintProject>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlueprintProject {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default)]
    pub activities: Vec<BlueprintActivity>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlueprintActivity {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<f64>,
}

fn default_country() -> String { "US".to_string() }
fn default_currency() -> String { "USD".to_string() }
fn default_timezone() -> String { "America/Chicago".to_string() }

// ── Entry points ──────────────────────────────────────────────────────────

pub async fn run(client: &KimaiAdminClient, from_file: Option<String>) -> Result<()> {
    let blueprint = match from_file {
        Some(path) => load_blueprint(&path)?,
        None => prompt_blueprint()?,
    };
    apply_blueprint(client, &blueprint).await
}

pub fn write_blank_blueprint(path: &str) -> Result<()> {
    let lines = [
        "# Kimai bootstrap blueprint",
        "# Run with: aw-kimai-admin bootstrap run --from-file bootstrap.toml",
        "",
        "[[customers]]",
        "name     = \"Acme Corp\"",
        "country  = \"US\"",
        "currency = \"USD\"",
        "timezone = \"America/Chicago\"",
        "email    = \"billing@acme.example\"",
        "budget   = 500.0",
        "",
        "  [[customers.projects]]",
        "  name   = \"Website Redesign\"",
        "  budget = 200.0",
        "  start  = \"2026-01-01\"",
        "  end    = \"2026-06-30\"",
        "",
        "    [[customers.projects.activities]]",
        "    name  = \"Frontend development\"",
        "",
        "    [[customers.projects.activities]]",
        "    name  = \"Backend development\"",
        "",
        "    [[customers.projects.activities]]",
        "    name  = \"Code review\"",
        "",
        "  [[customers.projects]]",
        "  name   = \"Mobile App\"",
        "  budget = 300.0",
        "",
        "    [[customers.projects.activities]]",
        "    name = \"iOS development\"",
        "",
        "    [[customers.projects.activities]]",
        "    name = \"Android development\"",
        "",
        "    [[customers.projects.activities]]",
        "    name = \"QA and Testing\"",
        "",
    ];
    std::fs::write(path, lines.join("\n"))
        .with_context(|| format!("Failed to write blueprint to {path}"))
}

// ── Blueprint loading ─────────────────────────────────────────────────────

fn load_blueprint(path: &str) -> Result<Blueprint> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read blueprint file: {path}"))?;
    toml::from_str(&raw)
        .with_context(|| format!("Failed to parse blueprint file: {path}"))
}

// ── Interactive prompt ────────────────────────────────────────────────────

fn prompt_blueprint() -> Result<Blueprint> {
    println!("\n=== Kimai Bootstrap Wizard ===");
    println!("Tip: use --from-file blueprint.toml for non-interactive setup.\n");
    let mut customers = vec![];
    loop {
        customers.push(prompt_customer()?);
        if !ask_yes_no("Add another customer?")? { break; }
    }
    Ok(Blueprint { customers })
}

fn prompt_customer() -> Result<BlueprintCustomer> {
    println!("\n-- New customer --");
    let name     = ask("Customer name")?;
    let country  = ask_with_default("Country (e.g. US)", "US")?;
    let currency = ask_with_default("Currency", "USD")?;
    let timezone = ask_with_default("Timezone", "America/Chicago")?;
    let email    = ask_optional("Contact email (optional)")?;
    let budget: Option<f64> = ask_optional("Budget hours (optional)")?
        .and_then(|s: String| s.parse().ok());
    let mut projects = vec![];
    if ask_yes_no(&format!("Add projects to \"{}\"?", name))? {
        loop {
            projects.push(prompt_project()?);
            if !ask_yes_no("Add another project?")? { break; }
        }
    }
    Ok(BlueprintCustomer { name, country, currency, timezone, email, budget, projects })
}

fn prompt_project() -> Result<BlueprintProject> {
    println!("\n  -- New project --");
    let name   = ask("  Project name")?;
    let budget: Option<f64> = ask_optional("  Budget hours (optional)")?
        .and_then(|s: String| s.parse().ok());
    let start = ask_optional("  Start date YYYY-MM-DD (optional)")?;
    let end   = ask_optional("  End date YYYY-MM-DD (optional)")?;
    let mut activities = vec![];
    if ask_yes_no(&format!("  Add activities to \"{}\"?", name))? {
        loop {
            activities.push(prompt_activity()?);
            if !ask_yes_no("  Add another activity?")? { break; }
        }
    }
    Ok(BlueprintProject { name, budget, start, end, activities })
}

fn prompt_activity() -> Result<BlueprintActivity> {
    println!("\n    -- New activity --");
    let name  = ask("    Activity name")?;
    let budget: Option<f64> = ask_optional("    Budget hours (optional)")?
        .and_then(|s: String| s.parse().ok());
    Ok(BlueprintActivity { name, budget })
}

// ── Apply blueprint ───────────────────────────────────────────────────────

pub async fn apply_blueprint(client: &KimaiAdminClient, bp: &Blueprint) -> Result<()> {
    println!("\nApplying blueprint to Kimai...\n");
    let (mut nc, mut np, mut na) = (0u32, 0u32, 0u32);
    for bc in &bp.customers {
        let customer = create_or_find_customer(client, bc).await?;
        nc += 1;
        println!("  + Customer  [{}] {}", customer.id, customer.name);
        for bp_proj in &bc.projects {
            let project = create_or_find_project(client, bp_proj, customer.id).await?;
            np += 1;
            println!("    + Project   [{}] {}", project.id, project.name);
            for bp_act in &bp_proj.activities {
                let activity = create_or_find_activity(client, bp_act, Some(project.id)).await?;
                na += 1;
                println!("      + Activity [{}] {}", activity.id, activity.name);
            }
        }
    }
    println!("\nDone -- {nc} customer(s), {np} project(s), {na} activity/activities.");
    println!("Copy the IDs above into your config.toml [[rules]] blocks.");
    Ok(())
}

async fn create_or_find_customer(client: &KimaiAdminClient, bc: &BlueprintCustomer) -> Result<Customer> {
    let existing = client.list_customers().await?;
    if let Some(c) = existing.iter().find(|c| c.name == bc.name) {
        println!("  ~ Customer already exists [{}] {}", c.id, c.name);
        return Ok(c.clone());
    }
    client.create_customer(super::CreateCustomerRequest {
        name: bc.name.clone(),
        country: bc.country.clone(),
        currency: bc.currency.clone(),
        timezone: bc.timezone.clone(),
        email: bc.email.clone(),
        color: None,
        budget: bc.budget,
        budget_type: None,
        visible: true,
    }).await
}

async fn create_or_find_project(
    client: &KimaiAdminClient,
    bp: &BlueprintProject,
    customer_id: u32,
) -> Result<Project> {
    let existing = client.list_projects(Some(customer_id)).await?;
    if let Some(p) = existing.iter().find(|p| p.name == bp.name) {
        println!("    ~ Project already exists [{}] {}", p.id, p.name);
        return Ok(p.clone());
    }
    client.create_project(super::CreateProjectRequest {
        name: bp.name.clone(),
        customer: customer_id,
        color: None,
        budget: bp.budget,
        budget_type: None,
        start: bp.start.clone(),
        end: bp.end.clone(),
        visible: true,
    }).await
}

async fn create_or_find_activity(
    client: &KimaiAdminClient,
    bp: &BlueprintActivity,
    project_id: Option<u32>,
) -> Result<Activity> {
    let existing = client.list_activities(project_id).await?;
    if let Some(a) = existing.iter().find(|a| a.name == bp.name) {
        println!("      ~ Activity already exists [{}] {}", a.id, a.name);
        return Ok(a.clone());
    }
    client.create_activity(super::CreateActivityRequest {
        name: bp.name.clone(),
        project: project_id,
        color: None,
        budget: bp.budget,
        budget_type: None,
        visible: true,
    }).await
}

// ── Prompt helpers ────────────────────────────────────────────────────────

fn ask(prompt: &str) -> Result<String> {
    print!("{}: ", prompt);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

fn ask_with_default(prompt: &str, default: &str) -> Result<String> {
    print!("{} [{}]: ", prompt, default);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let t = buf.trim().to_string();
    Ok(if t.is_empty() { default.to_string() } else { t })
}

fn ask_optional(prompt: &str) -> Result<Option<String>> {
    print!("{}: ", prompt);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let t = buf.trim().to_string();
    Ok(if t.is_empty() { None } else { Some(t) })
}

fn ask_yes_no(prompt: &str) -> Result<bool> {
    print!("{} [y/N] ", prompt);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().eq_ignore_ascii_case("y"))
}
