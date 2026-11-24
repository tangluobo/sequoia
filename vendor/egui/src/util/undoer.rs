use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Settings {
    /// Maximum number of undos.
    /// If your state is resource intensive, you should keep this low.
    ///
    /// Default: `100`
    pub max_undos: usize,

    /// When that state hasn't changed for this many seconds,
    /// create a new undo point (if one is needed).
    ///
    /// Default value: `1.0` seconds.
    pub stable_time: f32,

    /// If the state is changing so often that we never get to `stable_time`,
    /// then still create a save point every `auto_save_interval` seconds,
    /// so we have something to undo to.
    ///
    /// Default value: `30` seconds.
    pub auto_save_interval: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_undos: 100,
            stable_time: 1.0,
            auto_save_interval: 30.0,
        }
    }
}

/// Automatic undo system.
///
/// Every frame you feed it the most recent state.
/// The [`Undoer`] compares it with the latest undo point
/// and if there is a change it may create a new undo point.
///
/// [`Undoer`] follows two simple rules:
///
/// 1) If the state has changed since the latest undo point, but has
///    remained stable for `stable_time` seconds, an new undo point is created.
/// 2) If the state does not stabilize within `auto_save_interval` seconds, an undo point is created.
///
/// Rule 1) will make sure an undo point is not created until you _stop_ dragging that slider.
/// Rule 2) will make sure that you will get some undo points even if you are constantly changing the state.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Undoer<State> {
    settings: Settings,

    /// New undoes are added to the back.
    /// Two adjacent undo points are never equal.
    /// The latest undo point may (often) be the current state.
    undos: VecDeque<State>,

    /// Stores redos immediately after a sequence of undos.
    /// Gets cleared every time the state changes.
    /// Does not need to be a deque, because there can only be up to `undos.len()` redos,
    /// which is already limited to `settings.max_undos`.
    redos: Vec<State>,

    #[cfg_attr(feature = "serde", serde(skip))]
    flux: Option<Flux<State>>,
}

impl<State> core::fmt::Debug for Undoer<State> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let Self { undos, redos, .. } = self;
        f.debug_struct("Undoer")
            .field("undo count", &undos.len())
            .field("redo count", &redos.len())
            .finish()
    }
}

impl<State> Default for Undoer<State>
where
    State: Clone + PartialEq,
{
    #[inline]
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            undos: VecDeque::new(),
            redos: Vec::new(),
            flux: None,
        }
    }
}

/// Represents how the current state is changing
#[derive(Clone)]
struct Flux<State> {
    start_time: f64,
    latest_change_time: f64,
    latest_state: State,
}

impl<State> Undoer<State>
where
    State: Clone + PartialEq,
{
    /// Create a new [`Undoer`] with the given [`Settings`].
    pub fn with_settings(settings: Settings) -> Self {
        Self {
            settings,
            ..Default::default()
        }
    }

    /// Bytes retained by saved states, redo states, and the current edit flux.
    /// Container allocation overhead is not included.
    pub fn memory_usage(&self, size_of: impl Fn(&State) -> usize) -> usize {
        self.undos
            .iter()
            .chain(&self.redos)
            .chain(self.flux.iter().map(|flux| &flux.latest_state))
            .fold(0usize, |sum, state| sum.saturating_add(size_of(state)))
    }

    /// Discard the oldest undo points, then the farthest redo points, to fit a budget.
    /// No states are cloned. A state larger than the budget is discarded as well.
    pub fn trim_memory(&mut self, budget: usize, size_of: impl Fn(&State) -> usize) -> usize {
        let mut used = self.memory_usage(&size_of);
        while used > budget {
            let removed = if self.undos.len() > 1 {
                self.undos.pop_front()
            } else if !self.redos.is_empty() {
                Some(self.redos.remove(0))
            } else if let Some(state) = self.undos.pop_front() {
                Some(state)
            } else {
                self.flux.take().map(|flux| flux.latest_state)
            };
            let Some(state) = removed else {
                break;
            };
            used = used.saturating_sub(size_of(&state));
        }
        used
    }

    /// Do we have an undo point different from the given state?
    pub fn has_undo(&self, current_state: &State) -> bool {
        match self.undos.len() {
            0 => false,
            1 => self.undos.back() != Some(current_state),
            _ => true,
        }
    }

    pub fn has_redo(&self, current_state: &State) -> bool {
        !self.redos.is_empty() && self.undos.back() == Some(current_state)
    }

    /// Return true if the state is currently changing
    pub fn is_in_flux(&self) -> bool {
        self.flux.is_some()
    }

    pub fn undo(&mut self, current_state: &State) -> Option<&State> {
        if self.has_undo(current_state) {
            self.flux = None;

            if let Some(state) = self.undos.pop_back_if(|state| &*state == current_state) {
                self.redos.push(state);
            } else {
                self.redos.push(current_state.clone());
            }

            // Note: we keep the undo point intact.
            self.undos.back()
        } else {
            None
        }
    }

    pub fn redo(&mut self, current_state: &State) -> Option<&State> {
        if !self.undos.is_empty() && self.undos.back() != Some(current_state) {
            // state changed since the last undo, redos should be cleared.
            self.redos.clear();
            None
        } else if let Some(state) = self.redos.pop() {
            self.undos.push_back(state);
            self.undos.back()
        } else {
            None
        }
    }

    /// Undo content changes while retaining the saved cursor of the destination.
    pub fn undo_by(
        &mut self,
        current_state: &State,
        same: impl Fn(&State, &State) -> bool,
    ) -> Option<&State> {
        let target = self
            .undos
            .iter()
            .rposition(|state| !same(state, current_state))?;
        self.flux = None;
        self.undos.truncate(target + 1);
        self.redos.push(current_state.clone());
        self.undos.back()
    }

    /// Redo with the content projection used by [`Self::undo_by`].
    pub fn redo_by(
        &mut self,
        current_state: &State,
        same: impl Fn(&State, &State) -> bool,
    ) -> Option<&State> {
        if self
            .undos
            .back()
            .is_some_and(|state| !same(state, current_state))
        {
            self.redos.clear();
            return None;
        }
        while let Some(state) = self.redos.pop() {
            if !same(&state, current_state) {
                self.undos.push_back(state);
                return self.undos.back();
            }
        }
        None
    }

    /// Add an undo point if, and only if, there has been a change since the latest undo point.
    pub fn add_undo(&mut self, current_state: &State) {
        if self.undos.back() != Some(current_state) {
            self.undos.push_back(current_state.clone());
        }
        while self.undos.len() > self.settings.max_undos {
            self.undos.pop_front();
        }
        self.flux = None;
    }

    /// Retain an already owned snapshot without cloning its document again.
    pub fn add_undo_owned(&mut self, current_state: State) {
        if self.undos.back() != Some(&current_state) {
            self.undos.push_back(current_state);
        }
        while self.undos.len() > self.settings.max_undos {
            self.undos.pop_front();
        }
        self.flux = None;
    }

    /// Call this as often as you want (e.g. every frame)
    /// and [`Undoer`] will determine if a new undo point should be created.
    ///
    /// * `current_time`: current time in seconds.
    pub fn feed_state(&mut self, current_time: f64, current_state: &State) {
        self.feed_state_by(current_time, current_state, |left, right| left == right);
    }

    /// Ignore differences outside the editor's content when timing automatic saves.
    pub fn feed_state_by(
        &mut self,
        current_time: f64,
        current_state: &State,
        same: impl Fn(&State, &State) -> bool,
    ) {
        self.feed_state_lazy(
            current_time,
            |saved| same(saved, current_state),
            || current_state.clone(),
        );
    }

    /// Build an owned snapshot only when the history or an in-progress edit needs it.
    pub fn feed_state_lazy(
        &mut self,
        current_time: f64,
        is_current: impl Fn(&State) -> bool,
        make_current: impl FnOnce() -> State,
    ) {
        match self.undos.back() {
            None => {
                // First time feed_state is called.
                // always create an undo point:
                self.add_undo_owned(make_current());
            }
            Some(latest_undo) => {
                if is_current(latest_undo) {
                    self.flux = None;
                } else {
                    self.redos.clear();

                    match self.flux.as_mut() {
                        None => {
                            self.flux = Some(Flux {
                                start_time: current_time,
                                latest_change_time: current_time,
                                latest_state: make_current(),
                            });
                        }
                        Some(flux) => {
                            if is_current(&flux.latest_state) {
                                let time_since_latest_change =
                                    (current_time - flux.latest_change_time) as f32;
                                if time_since_latest_change >= self.settings.stable_time {
                                    self.add_undo_owned(make_current());
                                }
                            } else {
                                let time_since_flux_start = (current_time - flux.start_time) as f32;
                                if time_since_flux_start >= self.settings.auto_save_interval {
                                    self.add_undo_owned(make_current());
                                } else {
                                    flux.latest_change_time = current_time;
                                    flux.latest_state = make_current();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
