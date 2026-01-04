use ratatui::widgets::ListState;
use tui_input::Input;

use crate::config::{
    Config, ENV_AUTH_TOKEN, ENV_BASE_URL, ENV_DEFAULT_HAIKU_MODEL, ENV_DEFAULT_OPUS_MODEL,
    ENV_DEFAULT_SONNET_MODEL, Profile,
};

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
    SaveEdit,
    CancelEdit,
    ResetConfig,
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
    },
}

pub const EDIT_FIELD_API_KEY: usize = 0;
pub const EDIT_FIELD_URL: usize = 1;
pub const EDIT_FIELD_HAIKU: usize = 2;
pub const EDIT_FIELD_SONNET: usize = 3;
pub const EDIT_FIELD_OPUS: usize = 4;
pub const EDIT_FIELD_COUNT: usize = 5;

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

    /// Selected profile to launch (set when user confirms selection)
    pub selected_profile: Option<Profile>,

    /// Status message to display (errors, confirmations)
    pub status_message: Option<String>,

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
            api_key_input: Input::default(),
            url_input: Input::default(),
            haiku_model_input: Input::default(),
            sonnet_model_input: Input::default(),
            opus_model_input: Input::default(),
            reveal_api_key: false,
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
        if let Some(profile) = self.current_profile() {
            self.selected_profile = Some(profile.clone());
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
                    let api_key = env_value(profile, ENV_AUTH_TOKEN);
                    let url = env_value(profile, ENV_BASE_URL);
                    let haiku = env_value(profile, ENV_DEFAULT_HAIKU_MODEL);
                    let sonnet = env_value(profile, ENV_DEFAULT_SONNET_MODEL);
                    let opus = env_value(profile, ENV_DEFAULT_OPUS_MODEL);

                    self.api_key_input = Input::new(api_key);
                    self.url_input = Input::new(url);
                    self.haiku_model_input = Input::new(haiku);
                    self.sonnet_model_input = Input::new(sonnet);
                    self.opus_model_input = Input::new(opus);
                    self.reveal_api_key = false;
                    self.mode = AppMode::EditProfile {
                        focused_field: EDIT_FIELD_API_KEY,
                    };
                }
            }
            Action::SaveEdit => {
                if let AppMode::EditProfile { .. } = self.mode {
                    let api_key = self.api_key_input.value().to_string();
                    let url = self.url_input.value().to_string();
                    let haiku = self.haiku_model_input.value().to_string();
                    let sonnet = self.sonnet_model_input.value().to_string();
                    let opus = self.opus_model_input.value().to_string();

                    if let Some(i) = self.list_state.selected() {
                        if let Some(profile) = self.config.profiles.get_mut(i) {
                            let updates = [
                                (ENV_AUTH_TOKEN, api_key),
                                (ENV_BASE_URL, url),
                                (ENV_DEFAULT_HAIKU_MODEL, haiku),
                                (ENV_DEFAULT_SONNET_MODEL, sonnet),
                                (ENV_DEFAULT_OPUS_MODEL, opus),
                            ];
                            for (key, value) in updates {
                                profile.env.insert(key.to_string(), value);
                            }

                            if let Err(e) = self.config.save() {
                                self.status_message = Some(format!("Failed to save config: {}", e));
                            } else {
                                self.status_message =
                                    Some("Profile updated successfully".to_string());
                            }
                        }
                    }
                    self.mode = AppMode::Normal;
                }
            }
            Action::CancelEdit => {
                self.mode = AppMode::Normal;
            }
            Action::ResetConfig => {
                self.config = Config::create_default();
                if let Err(e) = self.config.save() {
                    self.status_message = Some(format!("Failed to reset config: {}", e));
                } else {
                    self.status_message = Some("Config reset to defaults".to_string());
                    let default_index = self.config.default_profile_index();
                    self.list_state.select(Some(default_index));
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
