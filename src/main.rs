use calamine::{open_workbook, Data, Range, Reader, Xlsx};
use clap::Parser;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use regex::Regex;
use rfd::{MessageDialog, MessageLevel, MessageButtons, MessageDialogResult};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::exit;

/// CLI tool to generate email drafts from an Excel file and a template.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Path to the Excel file (.xlsx)
    #[arg(short, long)]
    excel_path: Option<String>,

    /// Path to the text-based email template (.txt)
    #[arg(short, long)]
    template_path: Option<String>,
}

fn main() {
    // Parse command-line arguments.
    let args = Args::parse();

    // Get files either from command line arguments or GUI file pickers
    let (excel_path, template_path) = get_file_paths(args.excel_path, args.template_path);

    // Validate that input files exist.
    if let Err(e) = validate_files(&excel_path, &template_path) {
        show_error_and_exit(&e);
    }

    // Read the email template from file.
    let template_content = read_template(&template_path).unwrap_or_else(|e| {
        show_error_and_exit(&format!("Error reading template file: {}", e));
    });
    let (template_subject, template_body) = extract_subject_body(&template_content);

    // Open the XLSX workbook.
    let mut workbook: Xlsx<_> = open_workbook(&excel_path).unwrap_or_else(|e| {
        show_error_and_exit(&format!("Error opening Excel file: {}", e));
    });

    // Since there is only one sheet with the information, retrieve it directly.
    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() {
        show_error_and_exit("Error: No sheets found in Excel file.");
    }
    let sheet_name = &sheet_names[0];

    // Retrieve the sheet's range.
    let data_range = match workbook.worksheet_range(sheet_name) {
        Ok(r) => r,
        Err(e) => {
            show_error_and_exit(&format!("Error reading sheet '{}': {}", sheet_name, e));
        }
    };

    // Extract the header row and its index.
    let (header, header_row_index) = match extract_header(&data_range) {
        Ok((h, i)) => (h, i),
        Err(e) => {
            show_error_and_exit(&e);
        }
    };

    // Build a mapping from lowercased header names to their column indices.
    let header_map = build_header_map(&header);
    if !header_map.contains_key("email address") {
        show_error_and_exit("Error: Excel file must contain an 'email address' column (case-insensitive).");
    }

    // Identify columns that include "name" in their header (for {name} processing).
    let name_columns = find_name_columns(&header);

    // Process each data row and generate email drafts.
    process_rows(
        &data_range,
        &header,
        header_row_index,
        &name_columns,
        &template_subject,
        &template_body,
    );
}

/// Uses GUI dialogs to get file paths if not provided via command line.
fn get_file_paths(excel_path_arg: Option<String>, template_path_arg: Option<String>) -> (String, String) {
    let excel_path = match excel_path_arg {
        Some(path) => path,
        None => {
            let file_dialog = rfd::FileDialog::new()
                .set_title("Select Excel File")
                .add_filter("Excel Files", &["xlsx"])
                .set_directory(Path::new("/"));
            match file_dialog.pick_file() {
                Some(path) => path.to_string_lossy().to_string(),
                None => {
                    show_error_and_exit("No Excel file selected.");
                }
            }
        }
    };

    let template_path = match template_path_arg {
        Some(path) => path,
        None => {
            let file_dialog = rfd::FileDialog::new()
                .set_title("Select Email Template File")
                .add_filter("Text Files", &["txt"])
                .set_directory(Path::new(&excel_path).parent().unwrap_or_else(|| Path::new("/")));
            match file_dialog.pick_file() {
                Some(path) => path.to_string_lossy().to_string(),
                None => {
                    show_error_and_exit("No template file selected.");
                }
            }
        }
    };

    (excel_path, template_path)
}

/// Shows an error message box and exits the program.
fn show_error_and_exit(message: &str) -> ! {
    eprintln!("{}", message);
    MessageDialog::new()
        .set_title("Error")
        .set_description(message)
        .set_level(MessageLevel::Error)
        .set_buttons(MessageButtons::Ok)
        .show();
    exit(1);
}

/// Shows an information message box.
fn show_info(message: &str) {
    println!("{}", message);
    MessageDialog::new()
        .set_title("Information")
        .set_description(message)
        .set_level(MessageLevel::Info)
        .set_buttons(MessageButtons::Ok)
        .show();
}

/// Shows a confirmation dialog and returns the user's choice.
fn show_confirm(message: &str) -> bool {
    MessageDialog::new()
        .set_title("Confirm")
        .set_description(message)
        .set_level(MessageLevel::Info)
        .set_buttons(MessageButtons::YesNo)
        .show() == MessageDialogResult::Yes
}

/// Validates that both the Excel and template files exist.
fn validate_files(excel_path: &str, template_path: &str) -> Result<(), String> {
    if !Path::new(excel_path).exists() {
        return Err(format!("Error: Excel file '{}' does not exist.", excel_path));
    }
    if !Path::new(template_path).exists() {
        return Err(format!("Error: Template file '{}' does not exist.", template_path));
    }
    Ok(())
}

/// Reads the template file and returns its content as a String.
fn read_template(template_path: &str) -> Result<String, std::io::Error> {
    fs::read_to_string(template_path)
}

/// Extracts the subject and body from the template content.
/// If the first line starts with "Subject:", that line is treated as the subject.
fn extract_subject_body(template_content: &str) -> (String, String) {
    if template_content.trim_start().starts_with("Subject:") {
        let mut lines = template_content.lines();
        let subject_line = lines.next().unwrap();
        let subject = subject_line.trim_start_matches("Subject:").trim().to_string();
        let body = lines.collect::<Vec<_>>().join("\n");
        (subject, body)
    } else {
        (String::new(), template_content.to_string())
    }
}

/// Searches for the first non-empty row in the Excel range and returns it as the header.
/// Returns the header vector and its row index.
fn extract_header(range: &Range<Data>) -> Result<(Vec<String>, usize), String> {
    for (i, row) in range.rows().enumerate() {
        if row.iter().any(|cell| !cell.to_string().trim().is_empty()) {
            let header: Vec<String> = row
                .iter()
                .map(|cell| cell.to_string().trim().to_string())
                .collect();
            return Ok((header, i));
        }
    }
    Err("Error: Could not find header row in Excel file.".to_string())
}

/// Builds a HashMap mapping lowercased header names to their column indices.
fn build_header_map(header: &Vec<String>) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (idx, col_name) in header.iter().enumerate() {
        map.insert(col_name.to_lowercase(), idx);
    }
    map
}

/// Returns a vector of tuples (column index, header name) for headers containing "name" (case-insensitive).
fn find_name_columns(header: &Vec<String>) -> Vec<(usize, String)> {
    header
        .iter()
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains("name"))
        .map(|(idx, name)| (idx, name.clone()))
        .collect()
}

/// Determines the appropriate name to use based on available columns.
fn identify_name(
    row_data: &HashMap<String, String>,
    name_columns: &Vec<(usize, String)>,
    header: &Vec<String>,
    row_index: usize,
) -> String {
    if name_columns.is_empty() {
        return "".to_string();
    }

    if name_columns.len() == 1 {
        let (idx, _) = name_columns[0];
        let key = header[idx].to_lowercase();
        return row_data.get(&key).unwrap_or(&"".to_string()).clone();
    }

    let exact_matches: Vec<&(usize, String)> = name_columns
        .iter()
        .filter(|(_, col_name)| col_name.trim().to_lowercase() == "name")
        .collect();

    let first_name = if exact_matches.len() == 1 {
        let &(idx, _) = exact_matches[0];
        let key = header[idx].to_lowercase();
        row_data.get(&key).unwrap_or(&"".to_string()).clone()
    } else if exact_matches.len() > 1 {
        let error = format!(
            "Error (row {}): Multiple columns exactly named 'name' found.",
            row_index + 1
        );
        show_error_and_exit(&error);
    } else {
        let candidates: Vec<String> = name_columns
            .iter()
            .map(|(idx, _)| {
                let key = header[*idx].to_lowercase();
                row_data.get(&key).unwrap_or(&"".to_string()).clone()
            })
            .collect();
        if candidates.len() != 1 {
            let error = format!(
                "Error (row {}): Multiple name columns found with ambiguous first name.",
                row_index + 1
            );
            show_error_and_exit(&error);
        }
        candidates[0].clone()
    };

    let last_name_candidates: Vec<&(usize, String)> = name_columns
        .iter()
        .filter(|(_, col_name)| {
            let lower = col_name.to_lowercase();
            lower.contains("last") || lower.contains("surname")
        })
        .collect();

    if last_name_candidates.len() == 1 {
        let &(last_idx, _) = last_name_candidates[0];
        let key = header[last_idx].to_lowercase();
        let last_name = row_data.get(&key).unwrap_or(&"".to_string()).clone();
        match (first_name.is_empty(), last_name.is_empty()) {
            (true, true) => "".to_string(),
            (true, false) => last_name,
            (false, true) => first_name,
            (false, false) => format!("{} {}", first_name, last_name),
        }
    } else {
        first_name
    }
}

/// Iterates over each data row, performs placeholder substitution,
/// launches the default email client with the generated draft,
/// and asks the user if they want to continue to the next draft.
fn process_rows(
    range: &Range<Data>,
    header: &Vec<String>,
    header_row_index: usize,
    name_columns: &Vec<(usize, String)>,
    template_subject: &str,
    template_body: &str,
) {
    let placeholder_re = Regex::new(r"\{([^}]+)\}").unwrap();
    let mut processed_count = 0;
    let mut skipped_count = 0;

    for (i, row) in range.rows().enumerate().skip(header_row_index + 1) {
        if row.iter().all(|cell| cell.to_string().trim().is_empty()) {
            continue;
        }
        let row_data = process_data_row(row, header);
        let email = match row_data.get("email address") {
            Some(e) if !e.is_empty() => e.clone(),
            _ => {
                let warning = format!(
                    "Warning (row {}): 'email address' cell is empty or missing. Skipping row.",
                    i + 1
                );
                eprintln!("{}", warning);
                show_info(&warning);
                skipped_count += 1;
                continue;
            }
        };

        let mut substitution = row_data.clone();

        if placeholder_re.is_match("{name}") {
            let name_value = identify_name(&row_data, name_columns, header, i);
            if name_value.is_empty() {
                let warning = format!("Warning (row {}): Template requires {{name}} but no matching column was found or the name is empty.", i + 1);
                eprintln!("{}", warning);
                show_info(&warning);
            }
            substitution.insert("name".to_string(), name_value);
        }

        let final_subject = if !template_subject.is_empty() {
            substitute_placeholders(template_subject, &substitution, i + 1, &placeholder_re)
        } else {
            "".to_string()
        };
        let final_body = substitute_placeholders(template_body, &substitution, i + 1, &placeholder_re);

        if let Err(e) = create_email_draft(&email, &final_subject, &final_body) {
            let error = format!("Error opening email draft for {}: {}", email, e);
            eprintln!("{}", error);
            show_info(&error);
            skipped_count += 1;
            continue;
        }
        
        processed_count += 1;

        let message = format!("Email draft opened for {}.\nContinue to the next draft?", email);
        if !show_confirm(&message) {
            break;
        }
    }

    let summary = format!(
        "Process completed!\nProcessed: {} email drafts\nSkipped: {} rows",
        processed_count, skipped_count
    );
    show_info(&summary);
}

/// Maps each cell in the row to its corresponding header (lowercased) as a String.
fn process_data_row(row: &[Data], header: &Vec<String>) -> HashMap<String, String> {
    let mut data = HashMap::new();
    for (col_idx, cell) in row.iter().enumerate() {
        if let Some(col_name) = header.get(col_idx) {
            data.insert(col_name.to_lowercase(), cell.to_string().trim().to_string());
        }
    }
    data
}

/// Replaces placeholders (e.g. {order id}, {name}) in the given text using the substitution map.
/// If a placeholder is missing in the map, a warning is printed and it is replaced with an empty string.
fn substitute_placeholders(
    text: &str,
    substitution: &HashMap<String, String>,
    row_number: usize,
    re: &Regex,
) -> String {
    re.replace_all(text, |caps: &regex::Captures| {
        let placeholder = caps.get(1).unwrap().as_str().trim().to_lowercase();
        if let Some(replacement) = substitution.get(&placeholder) {
            replacement.to_string()
        } else {
            let warning = format!(
                "Warning (row {}): No matching column found for placeholder '{{{}}}'. Replacing with empty string.",
                row_number, placeholder
            );
            eprintln!("{}", warning);
            "".to_string()
        }
    })
    .to_string()
}

/// Constructs a mailto URL with URL-encoded subject and body, and launches the default email client.
fn create_email_draft(email: &str, subject: &str, body: &str) -> Result<(), String> {
    let mut mailto_url = format!("mailto:{}", email);
    let mut params: Vec<String> = Vec::new();
    if !subject.is_empty() {
        let encoded_subject = utf8_percent_encode(subject, NON_ALPHANUMERIC).to_string();
        params.push(format!("subject={}", encoded_subject));
    }
    if !body.is_empty() {
        let encoded_body = utf8_percent_encode(body, NON_ALPHANUMERIC).to_string();
        params.push(format!("body={}", encoded_body));
    }
    if !params.is_empty() {
        mailto_url.push_str("?");
        mailto_url.push_str(&params.join("&"));
    }
    match open::that(&mailto_url) {
        Ok(_) => {
            println!("Draft email opened for {}.", email);
            Ok(())
        }
        Err(e) => Err(format!("Failed to open email client: {}", e)),
    }
}
