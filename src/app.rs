use ratatui::widgets::ListState;
use std::collections::HashMap;
use tui_input::Input;

use crate::config::{
    Config, ENV_AUTH_TOKEN, ENV_BASE_URL, ENV_DEFAULT_HAIKU_MODEL, ENV_DEFAULT_OPUS_MODEL,
    ENV_DEFAULT_SONNET_MODEL, ENV_MODEL, ENV_SMALL_FAST_MODEL, Profile,
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
    DeleteProfile,
    SelectLMStudio,
    OpenLMStudio,
    BackToProfiles,
    ToggleAuxiliarySelection,
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
    LMStudioModelSelection,
}

pub const EDIT_FIELD_NAME: usize = 0;
pub const EDIT_FIELD_DESCRIPTION: usize = 1;
pub const EDIT_FIELD_API_KEY: usize = 2;
pub const EDIT_FIELD_URL: usize = 3;
pub const EDIT_FIELD_HAIKU: usize = 4;
pub const EDIT_FIELD_SONNET: usize = 5;
pub const EDIT_FIELD_OPUS: usize = 6;
pub const EDIT_FIELD_COUNT: usize = 7;

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

    /// Input for Haiku model
    pub haiku_model_input: Input,

    /// Input for Sonnet model
    pub sonnet_model_input: Input,

    /// Input for Opus model
    pub opus_model_input: Input,

    /// Whether to reveal the API key in the edit form
    pub reveal_api_key: bool,

    /// List of models from LMStudio
    pub lmstudio_models: Vec<String>,

    /// Selection state for LMStudio models
    pub lmstudio_list_state: ListState,

    /// Whether we're selecting the auxiliary model (true) or main model (false)
    pub selecting_auxiliary_model: bool,
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
            haiku_model_input: Input::default(),
            sonnet_model_input: Input::default(),
            opus_model_input: Input::default(),
            reveal_api_key: false,
            lmstudio_models: Vec::new(),
            lmstudio_list_state: ListState::default(),
            selecting_auxiliary_model: false,
        }
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
        match &self.mode {
            AppMode::LMStudioModelSelection => {
                if let Some(i) = self.lmstudio_list_state.selected()
                    && let Some(model_name) = self.lmstudio_models.get(i).cloned()
                {
                    // Update the "lmstudio" profile in config
                    if let Some(lmstudio_profile) = self
                        .config
                        .profiles
                        .iter_mut()
                        .find(|p| p.name == "lmstudio")
                    {
                        if self.selecting_auxiliary_model {
                            // Setting auxiliary model only
                            lmstudio_profile
                                .env
                                .insert(ENV_SMALL_FAST_MODEL.to_string(), model_name.clone());

                            // Update description to show both models
                            let main_model = lmstudio_profile
                                .env
                                .get(ENV_MODEL)
                                .cloned()
                                .unwrap_or_else(|| "none".to_string());
                            lmstudio_profile.description =
                                format!("Main: {} | Aux: {}", main_model, model_name);

                            if let Err(e) = self.config.save() {
                                self.status_message = Some(format!("Failed to save config: {}", e));
                            } else {
                                self.status_message =
                                    Some(format!("Set auxiliary model: {}", model_name));
                            }
                        } else {
                            // Setting main model - create full environment
                            let mut env = HashMap::new();
                            env.insert(
                                ENV_BASE_URL.to_string(),
                                proxy::PROXY_ANTHROPIC_URL.to_string(),
                            );
                            env.insert(ENV_AUTH_TOKEN.to_string(), "lmstudio".to_string());

                            // Set all models to the same LMStudio model
                            env.insert(ENV_DEFAULT_HAIKU_MODEL.to_string(), model_name.clone());
                            env.insert(ENV_DEFAULT_SONNET_MODEL.to_string(), model_name.clone());
                            env.insert(ENV_DEFAULT_OPUS_MODEL.to_string(), model_name.clone());
                            env.insert(ENV_MODEL.to_string(), model_name.clone());

                            // Preserve existing auxiliary model if set
                            if let Some(aux) =
                                lmstudio_profile.env.get(ENV_SMALL_FAST_MODEL).cloned()
                            {
                                env.insert(ENV_SMALL_FAST_MODEL.to_string(), aux.clone());
                                lmstudio_profile.description =
                                    format!("Main: {} | Aux: {}", model_name, aux);
                            } else {
                                lmstudio_profile.description =
                                    format!("LMStudio model: {}", model_name);
                            }

                            lmstudio_profile.env = env;

                            if let Err(e) = self.config.save() {
                                self.status_message = Some(format!("Failed to save config: {}", e));
                            } else {
                                self.status_message =
                                    Some(format!("Set main model: {}", model_name));
                            }
                        }
                    }

                    self.mode = AppMode::Normal;
                    self.selecting_auxiliary_model = false;
                }
            }
            AppMode::Normal => {
                if let Some(profile) = self.current_profile() {
                    if profile.name == "lmstudio" && profile.env.is_empty() {
                        self.status_message =
                            Some("Press 'l' to select an LMStudio model".to_string());
                    } else {
                        self.selected_profile = Some(profile.clone());
                    }
                }
            }
            _ => {}
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
                    let haiku = env_value(profile, ENV_DEFAULT_HAIKU_MODEL);
                    let sonnet = env_value(profile, ENV_DEFAULT_SONNET_MODEL);
                    let opus = env_value(profile, ENV_DEFAULT_OPUS_MODEL);

                    self.name_input = Input::new(name);
                    self.description_input = Input::new(description);
                    self.api_key_input = Input::new(api_key);
                    self.url_input = Input::new(url);
                    self.haiku_model_input = Input::new(haiku);
                    self.sonnet_model_input = Input::new(sonnet);
                    self.opus_model_input = Input::new(opus);
                    self.reveal_api_key = false;
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
                self.url_input = Input::default();
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
                    let haiku = self.haiku_model_input.value().to_string();
                    let sonnet = self.sonnet_model_input.value().to_string();
                    let opus = self.opus_model_input.value().to_string();

                    let updates = [
                        (ENV_AUTH_TOKEN, api_key),
                        (ENV_BASE_URL, url),
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
                            profile.env.insert(key.to_string(), value);
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
                    if let Some(default_profile) = default_config
                        .profiles
                        .into_iter()
                        .find(|p| p.name == name)
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
                self.config = Config::create_default();
                if let Err(e) = self.config.save() {
                    self.status_message = Some(format!("Failed to reset config: {}", e));
                } else {
                    self.status_message = Some("All profiles reset to defaults".to_string());
                    let default_index = self.config.default_profile_index();
                    self.list_state.select(Some(default_index));
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
            Action::SelectLMStudio => {
                self.fetch_lmstudio_models();
            }
            Action::OpenLMStudio => {
                // Open LMStudio application
                std::process::Command::new("open")
                    .args(["-a", "LM Studio"])
                    .spawn()
                    .ok();
                self.status_message = Some("Opening LMStudio...".to_string());
            }
            Action::BackToProfiles => {
                self.mode = AppMode::Normal;
                self.selecting_auxiliary_model = false;
            }
            Action::ToggleAuxiliarySelection => {
                if self.mode == AppMode::LMStudioModelSelection {
                    self.selecting_auxiliary_model = !self.selecting_auxiliary_model;
                }
            }
        }
    }

    pub fn fetch_lmstudio_models(&mut self) {
        self.status_message = Some("Fetching models from LMStudio...".to_string());
        match crate::lmstudio::list_local_models() {
            Ok(models) => {
                self.set_lmstudio_models(models);
                if self.lmstudio_models.is_empty() {
                    self.status_message =
                        Some("No models loaded. Press 'l' to open LMStudio.".to_string());
                } else {
                    self.status_message = None;
                }
            }
            Err(_) => {
                self.set_lmstudio_models(Vec::new());
                self.status_message =
                    Some("LMStudio server not running. Press 'l' to open LMStudio.".to_string());
            }
        }
        // Always enter model selection mode so user can press 'l' to open LMStudio
        self.mode = AppMode::LMStudioModelSelection;
    }

    fn set_lmstudio_models(&mut self, models: Vec<String>) {
        let previous_selected_index = self.lmstudio_list_state.selected();
        let previous_selected_model_id = previous_selected_index
            .and_then(|i| self.lmstudio_models.get(i))
            .cloned();

        self.lmstudio_models = models;

        if self.lmstudio_models.is_empty() {
            self.lmstudio_list_state.select(None);
            return;
        }

        if let Some(model_id) = previous_selected_model_id
            && let Some(index) = self.lmstudio_models.iter().position(|m| m == &model_id)
        {
            self.lmstudio_list_state.select(Some(index));
            return;
        }

        let fallback_index = previous_selected_index.unwrap_or(0);
        let clamped = fallback_index.min(self.lmstudio_models.len() - 1);
        self.lmstudio_list_state.select(Some(clamped));
    }

    fn move_selection(&mut self, delta: isize) {
        match &mut self.mode {
            AppMode::LMStudioModelSelection => {
                let len = self.lmstudio_models.len();
                if len == 0 {
                    return;
                }
                let current = self.lmstudio_list_state.selected().unwrap_or(0) as isize;
                let next = (current + delta).rem_euclid(len as isize) as usize;
                self.lmstudio_list_state.select(Some(next));
            }
            _ => {
                let len = self.config.profiles.len();
                if len == 0 {
                    return;
                }
                let current = self.list_state.selected().unwrap_or(0) as isize;
                let next = (current + delta).rem_euclid(len as isize) as usize;
                self.list_state.select(Some(next));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_lmstudio_models_preserves_selection_by_model_id_when_possible() {
        let mut app = App::new(Config::create_default());

        app.lmstudio_models = vec!["a".into(), "b".into(), "c".into()];
        app.lmstudio_list_state.select(Some(1));

        app.set_lmstudio_models(vec!["a".into(), "b".into(), "c".into(), "d".into()]);

        assert_eq!(app.lmstudio_list_state.selected(), Some(1));
    }

    #[test]
    fn set_lmstudio_models_preserves_selection_when_list_is_reordered() {
        let mut app = App::new(Config::create_default());

        app.lmstudio_models = vec!["a".into(), "b".into(), "c".into()];
        app.lmstudio_list_state.select(Some(1));

        app.set_lmstudio_models(vec!["b".into(), "c".into(), "a".into()]);

        assert_eq!(app.lmstudio_list_state.selected(), Some(0));
    }

    #[test]
    fn set_lmstudio_models_falls_back_to_clamped_index_when_selected_model_disappears() {
        let mut app = App::new(Config::create_default());

        app.lmstudio_models = vec!["a".into(), "b".into(), "c".into()];
        app.lmstudio_list_state.select(Some(2));

        app.set_lmstudio_models(vec!["x".into()]);

        assert_eq!(app.lmstudio_list_state.selected(), Some(0));
    }

    #[test]
    fn set_lmstudio_models_selects_first_item_when_no_previous_selection() {
        let mut app = App::new(Config::create_default());

        app.lmstudio_models = vec!["a".into(), "b".into()];
        app.lmstudio_list_state.select(None);

        app.set_lmstudio_models(vec!["a".into(), "b".into(), "c".into()]);

        assert_eq!(app.lmstudio_list_state.selected(), Some(0));
    }

    #[test]
    fn set_lmstudio_models_clears_selection_when_list_becomes_empty() {
        let mut app = App::new(Config::create_default());

        app.lmstudio_models = vec!["a".into()];
        app.lmstudio_list_state.select(Some(0));

        app.set_lmstudio_models(Vec::new());

        assert_eq!(app.lmstudio_list_state.selected(), None);
    }

    #[test]
    fn move_selection_wraps_profiles() {
        let mut app = App::new(Config::create_default());
        app.list_state.select(Some(0));

        app.move_selection(-1);

        let last_index = app.config.profiles.len() - 1;
        assert_eq!(app.list_state.selected(), Some(last_index));
    }

    #[test]
    fn move_selection_wraps_lmstudio_models() {
        let mut app = App::new(Config::create_default());
        app.mode = AppMode::LMStudioModelSelection;
        app.lmstudio_models = vec!["a".into(), "b".into(), "c".into()];
        app.lmstudio_list_state.select(Some(0));

        app.move_selection(-1);

        assert_eq!(app.lmstudio_list_state.selected(), Some(2));
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

        assert_eq!(app.config.profiles.len(), 4); // Default config has 4 profiles
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
        assert!(app.config.profiles.iter().all(|p| p.name != profile_to_delete));
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
}
