// src/bin/admin/output.rs

use crate::kimai_admin::{Activity, Customer, Project};
use crate::OutputFormat;


// ── Customers ──────────────────────────────────────────────────────────────

pub fn print_customers(items: &[Customer], fmt: &OutputFormat) {
    match fmt {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(items).unwrap()),
        OutputFormat::Table => {
            print_header(&["ID", "Name", "Currency", "Timezone", "Email", "Budget"]);
            for c in items {
                println!(
                    "{:<6} {:<30} {:<10} {:<25} {:<28} {}",
                    c.id,
                    truncate(&c.name, 29),
                    c.currency,
                    c.timezone,
                    c.email.as_deref().unwrap_or("-"),
                    c.budget.map(|b| format!("{b:.0}h")).unwrap_or_else(|| "-".to_string()),
                );
            }
            print_count(items.len(), "customer");
        }
    }
}

// ── Projects ───────────────────────────────────────────────────────────────

pub fn print_projects(items: &[Project], fmt: &OutputFormat) {
    match fmt {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(items).unwrap()),
        OutputFormat::Table => {
            print_header(&["ID", "Name", "Customer", "Budget", "Start", "End"]);
            for p in items {
                println!(
                    "{:<6} {:<30} {:<10} {:<10} {:<12} {}",
                    p.id,
                    truncate(&p.name, 29),
                    p.customer,
                    p.budget.map(|b| format!("{b:.0}h")).unwrap_or_else(|| "-".to_string()),
                    p.start.as_deref().unwrap_or("-"),
                    p.end.as_deref().unwrap_or("-"),
                );
            }
            print_count(items.len(), "project");
        }
    }
}

// ── Activities ─────────────────────────────────────────────────────────────

pub fn print_activities(items: &[Activity], fmt: &OutputFormat) {
    match fmt {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(items).unwrap()),
        OutputFormat::Table => {
            print_header(&["ID", "Name", "Project", "Budget"]);
            for a in items {
                println!(
                    "{:<6} {:<35} {:<10} {}",
                    a.id,
                    truncate(&a.name, 34),
                    a.project.map(|p: u32| p.to_string()).unwrap_or_else(|| "global".to_string()),
                    a.budget.map(|b| format!("{b:.0}h")).unwrap_or_else(|| "-".to_string()),
                );
            }
            print_count(items.len(), "activity");
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn print_header(cols: &[&str]) {
    let line: Vec<String> = cols.iter().map(|c| format!("{:<width$}", c, width = col_width(c))).collect();
    println!("{}", line.join(" "));
    println!("{}", "─".repeat(72));
}

fn col_width(col: &str) -> usize {
    match col {
        "ID"       => 6,
        "Name"     => 30,
        "Customer" => 10,
        "Budget"   => 10,
        "Start" | "End" => 12,
        "Currency" => 10,
        "Timezone" => 25,
        "Email"    => 28,
        "Project"  => 10,
        _          => 15,
    }
}

fn print_count(n: usize, entity: &str) {
    let label = if n == 1 {
        entity.to_string()
    } else {
        format!("{entity}s")
    };
    println!("\n{n} {label}");
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max - 1).collect();
        t.push('…');
        t
    }
}