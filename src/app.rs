use ratatui::widgets::ListState;
use tui_input::Input;

use crate::config::{Config, Profile};

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
        /// 0 for API Key, 1 for URL, 2 for Haiku, 3 for Sonnet, 4 for Opus
        focused_field: usize,
    },
}

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
        if self.config.profiles.is_empty() {
            return;
        }

        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.config.profiles.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    /// Move selection down in the profile list
    pub fn next(&mut self) {
        if self.config.profiles.is_empty() {
            return;
        }

        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.config.profiles.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
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
                    let api_key = profile
                        .env
                        .get("ANTHROPIC_AUTH_TOKEN")
                        .cloned()
                        .unwrap_or_default();
                    let url = profile
                        .env
                        .get("ANTHROPIC_BASE_URL")
                        .cloned()
                        .unwrap_or_default();
                    let haiku = profile
                        .env
                        .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                        .cloned()
                        .unwrap_or_default();
                    let sonnet = profile
                        .env
                        .get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                        .cloned()
                        .unwrap_or_default();
                    let opus = profile
                        .env
                        .get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                        .cloned()
                        .unwrap_or_default();

                    self.api_key_input = Input::new(api_key);
                    self.url_input = Input::new(url);
                    self.haiku_model_input = Input::new(haiku);
                    self.sonnet_model_input = Input::new(sonnet);
                    self.opus_model_input = Input::new(opus);
                    self.reveal_api_key = false;
                    self.mode = AppMode::EditProfile { focused_field: 0 };
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
                            profile
                                .env
                                .insert("ANTHROPIC_AUTH_TOKEN".to_string(), api_key);
                            profile.env.insert("ANTHROPIC_BASE_URL".to_string(), url);
                            profile
                                .env
                                .insert("ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(), haiku);
                            profile
                                .env
                                .insert("ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(), sonnet);
                            profile
                                .env
                                .insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), opus);

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
}
