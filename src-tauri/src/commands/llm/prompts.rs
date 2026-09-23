//! Push-based localization of the prompts the backend seeds.
//!
//! `default_roster()`, the team templates and the help assistant all start from
//! text that the user reads in the roster, so it has to follow the interface
//! language — but the seeds live in Rust, where `$t` is unreachable, and the
//! result is stored in `roster.json` as the user's own data.
//!
//! Same contract as `sync_tray_strings`: no language detection in the backend,
//! the frontend resolves the strings and pushes them on startup and after every
//! locale change. Until the first push the compiled-in English set is used, and
//! existing roster entries are never rewritten — the user may have edited them.

use omniget_core::core::llm::roster_store::{set_prompt_defaults, PromptDefaults};

#[tauri::command]
pub fn sync_llm_prompts(
    omni: String,
    builder: String,
    scout: String,
    template_chief: String,
    template_researcher: String,
    template_writer: String,
    template_em: String,
    template_reviewer: String,
    help: String,
) {
    set_prompt_defaults(PromptDefaults {
        omni,
        builder,
        scout,
        template_chief,
        template_researcher,
        template_writer,
        template_em,
        template_reviewer,
        help,
    });
}
