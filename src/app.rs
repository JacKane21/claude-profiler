use ratatui::widgets::ListState;
use std::collections::HashMap;
use tui_input::Input;

use crate::config::{
    Config, ENV_AUTH_TOKEN, ENV_BASE_URL, ENV_DEFAULT_HAIKU_MODEL, ENV_DEFAULT_OPUS_MODEL,
    ENV_DEFAULT_SONNET_MODEL, ENV_MODEL, ENV_PROXY_TARGET_URL, Profile,
};
use crate::proxy;

/// Possible application actions from user input
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    MoveUp,
    MoveDown,
    SelectProfile,
    ShowHelp,
    HideHelp,
    EditProfile,
    CreateProfile,
    SaveEdit,
    CancelEdit,
    ResetProfile,
    ResetAll,
    ResetOAuth,
    DeleteProfile,
}

/// Current application mode
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AppMode {
    #[default]
    Normal,
    Help,
    EditProfile {
        /// Index into edit fields (see EDIT_FIELD_* constants)
        focused_field: usize,
        /// Whether we are creating a new profile or editing an existing one
        is_creating: bool,
    },
    /// Model picker popup (shown over EditProfile)
    ModelPicker {
        /// Which model field triggered the picker (EDIT_FIELD_HAIKU, EDIT_FIELD_SONNET, or EDIT_FIELD_OPUS)
        target_field: usize,
        /// Whether we are creating a new profile
        is_creating: bool,
    },
}

pub const EDIT_FIELD_NAME: usize = 0;
pub const EDIT_FIELD_DESCRIPTION: usize = 1;
pub const EDIT_FIELD_API_KEY: usize = 2;
pub const EDIT_FIELD_URL: usize = 3;
pub const EDIT_FIELD_PROXY_URL: usize = 4;
pub const EDIT_FIELD_HAIKU: usize = 5;
pub const EDIT_FIELD_SONNET: usize = 6;
pub const EDIT_FIELD_OPUS: usize = 7;
pub const EDIT_FIELD_COUNT: usize = 8;

/// Main application state
pub struct App {
    /// Current mode/screen
    pub mode: AppMode,

    /// Loaded configuration
    pub config: Config,

    /// Profile list selection state (for ratatui StatefulWidget)
    pub list_state: ListState,

    /// Whether the app should exit
    pub should_quit: bool,

    /// Selected profile to launch (set when the user confirms selection)
    pub selected_profile: Option<Profile>,

    /// Status message to display (errors, confirmations)
    pub status_message: Option<String>,

    /// Input for Name
    pub name_input: Input,

    /// Input for Description
    pub description_input: Input,

    /// Input for API Key
    pub api_key_input: Input,

    /// Input for Base URL
    pub url_input: Input,

    /// Input for Proxy Target URL
    pub proxy_url_input: Input,

    /// Input for Haiku model
    pub haiku_model_input: Input,

    /// Input for Sonnet model
    pub sonnet_model_input: Input,

    /// Input for Opus model
    pub opus_model_input: Input,

    /// Whether to reveal the API key in the edit form
    pub reveal_api_key: bool,

    /// Available Codex models for the model picker
    pub codex_models: Vec<String>,

    /// Selected index in the model picker
    pub model_picker_index: usize,
}

fn env_value(profile: &Profile, key: &str) -> String {
    profile.env.get(key).cloned().unwrap_or_default()
}

impl App {
    pub fn new(config: Config) -> Self {
        let default_index = config.default_profile_index();
        let mut list_state = ListState::default();
        list_state.select(Some(default_index));

        Self {
            mode: AppMode::Normal,
            config,
            list_state,
            should_quit: false,
            selected_profile: None,
            status_message: None,
            name_input: Input::default(),
            description_input: Input::default(),
            api_key_input: Input::default(),
            url_input: Input::default(),
            proxy_url_input: Input::default(),
            haiku_model_input: Input::default(),
            sonnet_model_input: Input::default(),
            opus_model_input: Input::default(),
            reveal_api_key: false,
            codex_models: Vec::new(),
            model_picker_index: 0,
        }
    }

    /// Check if the current profile being edited is a Codex profile
    pub fn is_codex_profile(&self) -> bool {
        let proxy_url = self.proxy_url_input.value();
        proxy_url.contains("chatgpt.com/backend-api/codex")
    }

    /// Check if the currently selected profile (in the list) is a Codex profile
    pub fn is_selected_profile_codex(&self) -> bool {
        if let Some(profile) = self.current_profile() {
            if let Some(val) = profile.env.get("OPENAI_OAUTH") {
                if val == "1" || val.eq_ignore_ascii_case("true") {
                    return true;
                }
            }
            let proxy_url = env_value(profile, ENV_PROXY_TARGET_URL);
            return proxy_url.contains("chatgpt.com/backend-api/codex");
        }
        false
    }

    /// Load Codex models (call this when entering edit mode for a Codex profile)
    pub fn load_codex_models(&mut self) {
        use crate::codex_instructions::get_cached_codex_models;
        self.codex_models = get_cached_codex_models();
    }

    /// Open the model picker for a specific field
    pub fn open_model_picker(&mut self, field: usize, is_creating: bool) {
        // Find current model value and try to select it
        let current_model = match field {
            EDIT_FIELD_HAIKU => self.haiku_model_input.value(),
            EDIT_FIELD_SONNET => self.sonnet_model_input.value(),
            EDIT_FIELD_OPUS => self.opus_model_input.value(),
            _ => "",
        };

        // Find index of current model, or default to gpt-5.2-codex-medium
        self.model_picker_index = self
            .codex_models
            .iter()
            .position(|m| m == current_model)
            .or_else(|| {
                self.codex_models
                    .iter()
                    .position(|m| m == "gpt-5.2-codex-medium")
            })
            .unwrap_or(0);

        self.mode = AppMode::ModelPicker {
            target_field: field,
            is_creating,
        };
    }

    /// Select a model from the picker and return to edit mode
    pub fn select_model_from_picker(&mut self, target_field: usize, is_creating: bool) {
        if let Some(model) = self.codex_models.get(self.model_picker_index) {
            let model = model.clone();
            match target_field {
                EDIT_FIELD_HAIKU => self.haiku_model_input = Input::new(model),
                EDIT_FIELD_SONNET => self.sonnet_model_input = Input::new(model),
                EDIT_FIELD_OPUS => self.opus_model_input = Input::new(model),
                _ => {}
            }
        }
        self.mode = AppMode::EditProfile {
            focused_field: target_field,
            is_creating,
        };
    }

    /// Cancel the model picker and return to edit mode
    pub fn cancel_model_picker(&mut self, target_field: usize, is_creating: bool) {
        self.mode = AppMode::EditProfile {
            focused_field: target_field,
            is_creating,
        };
    }

    /// Move selection up in the profile list
    pub fn previous(&mut self) {
        self.move_selection(-1);
    }

    /// Move selection down in the profile list
    pub fn next(&mut self) {
        self.move_selection(1);
    }

    /// Get the currently highlighted profile
    pub fn current_profile(&self) -> Option<&Profile> {
        self.list_state
            .selected()
            .and_then(|i| self.config.profiles.get(i))
    }

    /// Confirm selection and prepare to launch
    pub fn select_current(&mut self) {
        if let AppMode::Normal = &self.mode {
            if let Some(profile) = self.current_profile() {
                self.selected_profile = Some(profile.clone());
            }
        }
    }

    /// Handle an action
    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.should_quit = true;
            }
            Action::MoveUp => {
                self.previous();
            }
            Action::MoveDown => {
                self.next();
            }
            Action::SelectProfile => {
                self.select_current();
            }
            Action::ShowHelp => {
                self.mode = AppMode::Help;
            }
            Action::HideHelp => {
                self.mode = AppMode::Normal;
            }
            Action::EditProfile => {
                if let Some(profile) = self.current_profile() {
                    let name = profile.name.clone();
                    let description = profile.description.clone();
                    let api_key = env_value(profile, ENV_AUTH_TOKEN);
                    let url = env_value(profile, ENV_BASE_URL);
                    let proxy_url = env_value(profile, ENV_PROXY_TARGET_URL);
                    
                    let fallback_model = env_value(profile, ENV_MODEL);
                    let haiku = profile
                        .env
                        .get(ENV_DEFAULT_HAIKU_MODEL)
                        .cloned()
                        .unwrap_or_else(|| fallback_model.clone());
                    let sonnet = profile
                        .env
                        .get(ENV_DEFAULT_SONNET_MODEL)
                        .cloned()
                        .unwrap_or_else(|| fallback_model.clone());
                    let opus = profile
                        .env
                        .get(ENV_DEFAULT_OPUS_MODEL)
                        .cloned()
                        .unwrap_or(fallback_model);

                    self.name_input = Input::new(name);
                    self.description_input = Input::new(description);
                    self.api_key_input = Input::new(api_key);
                    self.url_input = Input::new(url);
                    self.proxy_url_input = Input::new(proxy_url.clone());
                    self.haiku_model_input = Input::new(haiku);
                    self.sonnet_model_input = Input::new(sonnet);
                    self.opus_model_input = Input::new(opus);
                    self.reveal_api_key = false;

                    // Load Codex models if this is a Codex profile
                    if proxy_url.contains("chatgpt.com/backend-api/codex") {
                        self.load_codex_models();
                    }

                    self.mode = AppMode::EditProfile {
                        focused_field: EDIT_FIELD_NAME,
                        is_creating: false,
                    };
                }
            }
            Action::CreateProfile => {
                self.name_input = Input::new("new-profile".to_string());
                self.description_input = Input::new("My custom profile".to_string());
                self.api_key_input = Input::default();
                self.url_input = Input::new(proxy::PROXY_ANTHROPIC_URL.to_string());
                self.proxy_url_input = Input::default();
                self.haiku_model_input = Input::default();
                self.sonnet_model_input = Input::default();
                self.opus_model_input = Input::default();
                self.reveal_api_key = false;
                self.mode = AppMode::EditProfile {
                    focused_field: EDIT_FIELD_NAME,
                    is_creating: true,
                };
            }
            Action::SaveEdit => {
                if let AppMode::EditProfile { is_creating, .. } = self.mode {
                    let name = self.name_input.value().to_string();
                    let description = self.description_input.value().to_string();
                    let api_key = self.api_key_input.value().to_string();
                    let url = self.url_input.value().to_string();
                    let proxy_url = self.proxy_url_input.value().to_string();
                    let haiku = self.haiku_model_input.value().to_string();
                    let sonnet = self.sonnet_model_input.value().to_string();
                    let opus = self.opus_model_input.value().to_string();

                    let updates = [
                        (ENV_AUTH_TOKEN, api_key),
                        (ENV_BASE_URL, url),
                        (ENV_PROXY_TARGET_URL, proxy_url),
                        (ENV_DEFAULT_HAIKU_MODEL, haiku),
                        (ENV_DEFAULT_SONNET_MODEL, sonnet),
                        (ENV_DEFAULT_OPUS_MODEL, opus),
                    ];

                    if is_creating {
                        let mut env = HashMap::new();
                        for (key, value) in updates {
                            if !value.is_empty() {
                                env.insert(key.to_string(), value);
                            }
                        }
                        let new_profile = Profile {
                            name: name.clone(),
                            description,
                            env,
                        };
                        self.config.profiles.push(new_profile);
                        self.status_message = Some(format!("Profile '{}' created", name));
                        // Select the newly created profile
                        self.list_state.select(Some(self.config.profiles.len() - 1));
                    } else if let Some(i) = self.list_state.selected()
                        && let Some(profile) = self.config.profiles.get_mut(i)
                    {
                        profile.name = name;
                        profile.description = description;
                        for (key, value) in updates {
                            if value.is_empty() {
                                profile.env.remove(key);
                            } else {
                                profile.env.insert(key.to_string(), value);
                            }
                        }
                        self.status_message = Some("Profile updated successfully".to_string());
                    }

                    if let Err(e) = self.config.save() {
                        self.status_message = Some(format!("Failed to save config: {}", e));
                    }
                    self.mode = AppMode::Normal;
                }
            }
            Action::CancelEdit => {
                self.mode = AppMode::Normal;
            }
            Action::ResetProfile => {
                if let Some(i) = self.list_state.selected() {
                    let profile = &mut self.config.profiles[i];
                    let name = profile.name.clone();

                    let default_config = Config::create_default();
                    if let Some(default_profile) =
                        default_config.profiles.into_iter().find(|p| p.name == name)
                    {
                        self.config.profiles[i] = default_profile;
                        self.status_message = Some(format!("Profile '{}' reset to default", name));
                    } else {
                        profile.env.clear();
                        self.status_message =
                            Some(format!("Profile '{}' environment cleared", name));
                    }

                    if let Err(e) = self.config.save() {
                        self.status_message = Some(format!("Failed to save config: {}", e));
                    }
                }
            }
            Action::ResetAll => {
                // Clear OAuth tokens first
                let _ = crate::openai_oauth::clear_tokens();

                self.config = Config::create_default();
                if let Err(e) = self.config.save() {
                    self.status_message = Some(format!("Failed to reset config: {}", e));
                } else {
                    self.status_message = Some("All profiles and OAuth tokens reset".to_string());
                    let default_index = self.config.default_profile_index();
                    self.list_state.select(Some(default_index));
                }
            }
            Action::ResetOAuth => {
                if let Err(e) = crate::openai_oauth::clear_tokens() {
                    self.status_message = Some(format!("Failed to clear OAuth tokens: {}", e));
                } else {
                    self.status_message = Some("OAuth tokens cleared. Sign in again on launch.".to_string());
                }
            }
            Action::DeleteProfile => {
                if let Some(i) = self.list_state.selected() {
                    let name = self.config.profiles[i].name.clone();
                    self.config.profiles.remove(i);
                    self.status_message = Some(format!("Profile '{}' deleted", name));

                    let len = self.config.profiles.len();
                    if len == 0 {
                        self.list_state.select(None);
                    } else if i >= len {
                        self.list_state.select(Some(len - 1));
                    }

                    if let Err(e) = self.config.save() {
                        self.status_message = Some(format!("Failed to save config: {}", e));
                    }
                }
            }
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.config.profiles.len();
        if len == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(len as isize) as usize;
        self.list_state.select(Some(next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_selection_wraps_profiles() {
        let mut app = App::new(Config::create_default());
        app.list_state.select(Some(0));

        app.move_selection(-1);

        let last_index = app.config.profiles.len() - 1;
        assert_eq!(app.list_state.selected(), Some(last_index));
    }

    #[test]
    fn reset_profile_restores_default_profile() {
        let mut app = App::new(Config::create_default());
        // 'zai' is index 1
        app.list_state.select(Some(1));

        // Modify zai profile
        app.config.profiles[1].description = "Modified".to_string();
        app.config.profiles[1]
            .env
            .insert("NEW_KEY".to_string(), "VALUE".to_string());

        app.handle_action(Action::ResetProfile);

        assert_eq!(
            app.config.profiles[1].description,
            "Z.ai API proxy (edit profiles.toml to add your API key)"
        );
        assert!(!app.config.profiles[1].env.contains_key("NEW_KEY"));
    }

    #[test]
    fn reset_profile_clears_custom_profile() {
        let mut app = App::new(Config::create_default());
        let custom_profile = Profile {
            name: "custom".to_string(),
            description: "Custom".to_string(),
            env: HashMap::from([("KEY".to_string(), "VALUE".to_string())]),
        };
        app.config.profiles.push(custom_profile);
        let custom_index = app.config.profiles.len() - 1;
        app.list_state.select(Some(custom_index));

        app.handle_action(Action::ResetProfile);

        assert_eq!(app.config.profiles[custom_index].name, "custom");
        assert!(app.config.profiles[custom_index].env.is_empty());
    }

    #[test]
    fn reset_all_restores_default_config() {
        let mut app = App::new(Config::create_default());
        app.config.profiles.clear();
        app.config.profiles.push(Profile {
            name: "temporary".to_string(),
            description: String::new(),
            env: HashMap::new(),
        });

        app.handle_action(Action::ResetAll);

        assert_eq!(app.config.profiles.len(), 6); // Default config has 6 profiles
        assert_eq!(app.config.profiles[0].name, "default");
    }

    #[test]
    fn delete_profile_removes_profile_and_adjusts_selection() {
        let mut app = App::new(Config::create_default());
        let initial_len = app.config.profiles.len();
        // Select 'zai' at index 1
        app.list_state.select(Some(1));
        let profile_to_delete = app.config.profiles[1].name.clone();

        app.handle_action(Action::DeleteProfile);

        assert_eq!(app.config.profiles.len(), initial_len - 1);
        assert!(
            app.config
                .profiles
                .iter()
                .all(|p| p.name != profile_to_delete)
        );
        // Selection should still be 1 (now pointing to 'minimax')
        assert_eq!(app.list_state.selected(), Some(1));
        assert_eq!(app.config.profiles[1].name, "minimax");
    }

    #[test]
    fn delete_last_profile_adjusts_selection() {
        let mut app = App::new(Config::create_default());
        let last_index = app.config.profiles.len() - 1;
        app.list_state.select(Some(last_index));

        app.handle_action(Action::DeleteProfile);

        assert_eq!(app.list_state.selected(), Some(last_index - 1));
    }

    #[test]
    fn edit_profile_falls_back_to_generic_model() {
        let mut app = App::new(Config::create_default());
        let custom_profile = Profile {
            name: "fallback_test".to_string(),
            description: "Test".to_string(),
            env: HashMap::from([(ENV_MODEL.to_string(), "fallback-model".to_string())]),
        };
        app.config.profiles.push(custom_profile);
        let custom_index = app.config.profiles.len() - 1;
        app.list_state.select(Some(custom_index));

        app.handle_action(Action::EditProfile);

        assert_eq!(app.haiku_model_input.value(), "fallback-model");
        assert_eq!(app.sonnet_model_input.value(), "fallback-model");
        assert_eq!(app.opus_model_input.value(), "fallback-model");
    }

    #[test]
    fn is_selected_profile_codex_detects_via_env_var() {
        let mut app = App::new(Config::create_default());
        let mut env = HashMap::new();
        env.insert("OPENAI_OAUTH".to_string(), "1".to_string());
        
        let profile = Profile {
            name: "codex-test".to_string(),
            description: "Test".to_string(),
            env,
        };
        app.config.profiles.push(profile);
        app.list_state.select(Some(app.config.profiles.len() - 1));

        assert!(app.is_selected_profile_codex());
    }
}
