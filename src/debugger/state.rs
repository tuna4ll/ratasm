//! The debugger state machine.
//!
//! ```text
//!                 ┌──────────────────────────────┐
//!                 ▼                              │
//!  Idle ──build──► Building ──ok──► Ready ──launch──► Starting
//!                     │                ▲                 │
//!                   failed             │              started
//!                     ▼                │                 ▼
//!                  Failed              │            Running ⇄ Paused
//!                     │                │                 │       │
//!                     └──reset─────────┘                exited───┘
//!                                      │                 ▼
//!                                      └──────────── Exited
//! ```
//!
//! # Why this is explicit
//!
//! The debugger's operations are only valid in particular states: stepping a
//! program that is not running, or setting a register while the target is
//! executing, produce confusing errors from GDB rather than clear ones from
//! us. Encoding the legal transitions in one place means the UI can *ask*
//! whether an action is possible — [`DebuggerState::can_step`] and friends —
//! and a mistake becomes an [`InvalidTransition`] naming both states rather
//! than a mysterious protocol failure.

use std::fmt;

/// Where the debugger currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum DebuggerState {
    /// Nothing is loaded and nothing is running.
    #[default]
    Idle,
    /// The project is being assembled and linked.
    Building,
    /// A debuggable executable exists but no session is running.
    Ready,
    /// GDB is starting and loading the program.
    Starting,
    /// The program is executing; the debugger cannot inspect it.
    Running,
    /// The program is stopped and can be inspected and stepped.
    Paused,
    /// The program finished.
    Exited,
    /// Something went wrong and the session cannot continue.
    Failed,
}

impl DebuggerState {
    /// Every state, for exhaustive testing and display.
    pub const ALL: [DebuggerState; 8] = [
        DebuggerState::Idle,
        DebuggerState::Building,
        DebuggerState::Ready,
        DebuggerState::Starting,
        DebuggerState::Running,
        DebuggerState::Paused,
        DebuggerState::Exited,
        DebuggerState::Failed,
    ];

    /// A short name for the status bar.
    pub const fn label(self) -> &'static str {
        match self {
            DebuggerState::Idle => "idle",
            DebuggerState::Building => "building",
            DebuggerState::Ready => "ready",
            DebuggerState::Starting => "starting",
            DebuggerState::Running => "running",
            DebuggerState::Paused => "paused",
            DebuggerState::Exited => "exited",
            DebuggerState::Failed => "failed",
        }
    }

    /// Whether a debug session exists, whether or not it is stopped.
    pub const fn is_session_active(self) -> bool {
        matches!(
            self,
            DebuggerState::Starting | DebuggerState::Running | DebuggerState::Paused
        )
    }

    /// Whether the program is stopped and inspectable.
    ///
    /// Reading registers, memory, the stack or the disassembly is only
    /// meaningful here: while the program runs, any value read is stale before
    /// it can be displayed.
    pub const fn can_inspect(self) -> bool {
        matches!(self, DebuggerState::Paused)
    }

    /// Whether a step command is currently valid.
    pub const fn can_step(self) -> bool {
        matches!(self, DebuggerState::Paused)
    }

    /// Whether execution can be resumed.
    pub const fn can_continue(self) -> bool {
        matches!(self, DebuggerState::Paused)
    }

    /// Whether the program can be interrupted.
    pub const fn can_interrupt(self) -> bool {
        matches!(self, DebuggerState::Running)
    }

    /// Whether a new session can be launched.
    pub const fn can_launch(self) -> bool {
        matches!(
            self,
            DebuggerState::Ready | DebuggerState::Exited | DebuggerState::Failed
        )
    }

    /// Whether a build can be started.
    ///
    /// Building while a session holds the executable open would replace the
    /// file underneath the running debugger, so it is refused until the
    /// session ends.
    pub const fn can_build(self) -> bool {
        matches!(
            self,
            DebuggerState::Idle
                | DebuggerState::Ready
                | DebuggerState::Exited
                | DebuggerState::Failed
        )
    }

    /// Whether the session can be stopped.
    pub const fn can_stop(self) -> bool {
        self.is_session_active()
    }

    /// Whether breakpoints can be edited right now.
    ///
    /// Before a session starts they are recorded and applied on launch; while
    /// paused GDB accepts them directly. While the program runs they are
    /// deferred, so the UI disables the action rather than appearing to work.
    pub const fn can_edit_breakpoints(self) -> bool {
        !matches!(self, DebuggerState::Running | DebuggerState::Starting)
    }
}

impl fmt::Display for DebuggerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Something that happens to the debugger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transition {
    /// A build was started.
    BuildStarted,
    /// The build produced an executable.
    BuildSucceeded,
    /// The build failed.
    BuildFailed,
    /// A debug session was requested.
    LaunchRequested,
    /// GDB loaded the program and it began executing.
    LaunchSucceeded,
    /// GDB could not start the session.
    LaunchFailed,
    /// The program stopped, at a breakpoint, a step, or a signal.
    Stopped,
    /// The program resumed.
    Resumed,
    /// The program finished.
    ProgramExited,
    /// The session broke: GDB died, or the protocol desynchronised.
    SessionFailed,
    /// The user dismissed a finished or failed session.
    Reset,
}

impl Transition {
    /// A short description for logs and error messages.
    pub const fn label(self) -> &'static str {
        match self {
            Transition::BuildStarted => "build started",
            Transition::BuildSucceeded => "build succeeded",
            Transition::BuildFailed => "build failed",
            Transition::LaunchRequested => "launch requested",
            Transition::LaunchSucceeded => "launch succeeded",
            Transition::LaunchFailed => "launch failed",
            Transition::Stopped => "program stopped",
            Transition::Resumed => "program resumed",
            Transition::ProgramExited => "program exited",
            Transition::SessionFailed => "session failed",
            Transition::Reset => "reset",
        }
    }
}

/// A transition that is not legal from the current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cannot apply '{transition}' while {from}", transition = transition.label())]
pub struct InvalidTransition {
    /// The state the machine was in.
    pub from: DebuggerState,
    /// The transition that was attempted.
    pub transition: Transition,
}

/// The debugger's state, with legal transitions enforced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateMachine {
    state: DebuggerState,
    /// The reason recorded with the most recent failure.
    failure: Option<String>,
    /// The exit status of the most recent run.
    exit_code: Option<i32>,
}

impl StateMachine {
    /// Creates a machine in [`DebuggerState::Idle`].
    pub fn new() -> Self {
        Self::default()
    }

    /// The current state.
    pub fn state(&self) -> DebuggerState {
        self.state
    }

    /// The reason for the most recent failure, if any.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// The exit code of the most recent run, if it exited normally.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Whether `transition` is legal from the current state.
    pub fn allows(&self, transition: Transition) -> bool {
        next_state(self.state, transition).is_some()
    }

    /// Applies `transition`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTransition`] when the transition is not legal from the
    /// current state, leaving the state unchanged.
    pub fn apply(&mut self, transition: Transition) -> Result<DebuggerState, InvalidTransition> {
        let next = next_state(self.state, transition).ok_or(InvalidTransition {
            from: self.state,
            transition,
        })?;

        match transition {
            Transition::BuildStarted | Transition::LaunchRequested | Transition::Reset => {
                self.failure = None;
                self.exit_code = None;
            }
            Transition::ProgramExited => self.failure = None,
            _ => {}
        }

        self.state = next;
        Ok(next)
    }

    /// Applies a failing transition, recording why.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTransition`] when the transition is not legal.
    pub fn fail(
        &mut self,
        transition: Transition,
        reason: impl Into<String>,
    ) -> Result<DebuggerState, InvalidTransition> {
        let next = self.apply(transition)?;
        self.failure = Some(reason.into());
        Ok(next)
    }

    /// Records that the program exited with `code`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTransition`] when the program was not running.
    pub fn exited(&mut self, code: Option<i32>) -> Result<DebuggerState, InvalidTransition> {
        let next = self.apply(Transition::ProgramExited)?;
        self.exit_code = code;
        Ok(next)
    }

    /// Forces the machine into [`DebuggerState::Failed`].
    ///
    /// Used when the GDB process dies unexpectedly, which can happen from any
    /// state and must always be representable — refusing the transition would
    /// leave the UI claiming a session that no longer exists.
    pub fn force_failed(&mut self, reason: impl Into<String>) {
        self.state = DebuggerState::Failed;
        self.failure = Some(reason.into());
    }
}

/// The state `transition` leads to, or `None` when it is not legal.
///
/// This function *is* the state diagram; everything else defers to it.
fn next_state(from: DebuggerState, transition: Transition) -> Option<DebuggerState> {
    use DebuggerState as S;
    use Transition as T;

    match (from, transition) {
        // Building can begin whenever no session holds the executable.
        (state, T::BuildStarted) if state.can_build() => Some(S::Building),
        (S::Building, T::BuildSucceeded) => Some(S::Ready),
        (S::Building, T::BuildFailed) => Some(S::Failed),

        // Launching.
        (state, T::LaunchRequested) if state.can_launch() => Some(S::Starting),
        (S::Starting, T::LaunchSucceeded) => Some(S::Running),
        (S::Starting, T::LaunchFailed) => Some(S::Failed),

        // GDB reports the first stop before the program has really run, so a
        // stop directly out of Starting is normal rather than an error.
        (S::Starting | S::Running, T::Stopped) => Some(S::Paused),
        (S::Paused, T::Resumed) => Some(S::Running),

        // A program can finish from any live state.
        (S::Starting | S::Running | S::Paused, T::ProgramExited) => Some(S::Exited),

        // A session can break at any point while it exists.
        (state, T::SessionFailed) if state.is_session_active() => Some(S::Failed),

        // Dismissing a finished or failed session returns to a usable state.
        (S::Exited | S::Failed, T::Reset) => Some(S::Idle),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DebuggerState as S;
    use Transition as T;

    fn machine_in(state: S) -> StateMachine {
        StateMachine {
            state,
            failure: None,
            exit_code: None,
        }
    }

    #[test]
    fn a_new_machine_is_idle() {
        let machine = StateMachine::new();
        assert_eq!(machine.state(), S::Idle);
        assert!(machine.failure().is_none());
        assert!(machine.exit_code().is_none());
    }

    #[test]
    fn the_happy_path_runs_end_to_end() {
        let mut machine = StateMachine::new();
        assert_eq!(machine.apply(T::BuildStarted), Ok(S::Building));
        assert_eq!(machine.apply(T::BuildSucceeded), Ok(S::Ready));
        assert_eq!(machine.apply(T::LaunchRequested), Ok(S::Starting));
        assert_eq!(machine.apply(T::LaunchSucceeded), Ok(S::Running));
        assert_eq!(machine.apply(T::Stopped), Ok(S::Paused));
        assert_eq!(machine.apply(T::Resumed), Ok(S::Running));
        assert_eq!(machine.exited(Some(0)), Ok(S::Exited));
        assert_eq!(machine.exit_code(), Some(0));
    }

    #[test]
    fn stepping_repeatedly_alternates_between_running_and_paused() {
        let mut machine = machine_in(S::Paused);
        for _ in 0..5 {
            assert_eq!(machine.apply(T::Resumed), Ok(S::Running));
            assert_eq!(machine.apply(T::Stopped), Ok(S::Paused));
        }
    }

    #[test]
    fn a_stop_arriving_during_startup_is_accepted() {
        // GDB reports the initial stop before the program has really begun.
        let mut machine = machine_in(S::Starting);
        assert_eq!(machine.apply(T::Stopped), Ok(S::Paused));
    }

    #[test]
    fn stepping_a_program_that_is_not_paused_is_refused() {
        for state in [S::Idle, S::Building, S::Ready, S::Running, S::Exited] {
            assert!(!state.can_step(), "{state} must not allow stepping");
            let mut machine = machine_in(state);
            assert!(machine.apply(T::Resumed).is_err(), "{state}");
        }
    }

    #[test]
    fn an_invalid_transition_leaves_the_state_untouched() {
        let mut machine = machine_in(S::Idle);
        let error = machine.apply(T::Stopped).expect_err("must be refused");
        assert_eq!(error.from, S::Idle);
        assert_eq!(error.transition, T::Stopped);
        assert_eq!(machine.state(), S::Idle, "state must not have moved");
    }

    #[test]
    fn the_error_message_names_the_state_and_the_action() {
        let error = InvalidTransition {
            from: S::Running,
            transition: T::Stopped,
        };
        let text = error.to_string();
        assert!(text.contains("running"), "{text}");
        assert!(text.contains("program stopped"), "{text}");
    }

    #[test]
    fn building_is_refused_while_a_session_holds_the_executable() {
        // Rebuilding under a live session would replace the file GDB has open.
        for state in [S::Starting, S::Running, S::Paused] {
            assert!(!state.can_build(), "{state} must not allow a build");
            let mut machine = machine_in(state);
            assert!(machine.apply(T::BuildStarted).is_err(), "{state}");
        }
    }

    #[test]
    fn building_is_allowed_once_a_session_has_finished() {
        for state in [S::Idle, S::Ready, S::Exited, S::Failed] {
            assert!(state.can_build(), "{state} should allow a build");
            let mut machine = machine_in(state);
            assert_eq!(machine.apply(T::BuildStarted), Ok(S::Building));
        }
    }

    #[test]
    fn a_failed_build_records_its_reason() {
        let mut machine = StateMachine::new();
        machine.apply(T::BuildStarted).expect("build starts");
        machine
            .fail(T::BuildFailed, "nasm: 3 errors")
            .expect("build fails");
        assert_eq!(machine.state(), S::Failed);
        assert_eq!(machine.failure(), Some("nasm: 3 errors"));
    }

    #[test]
    fn starting_a_new_build_clears_the_previous_failure() {
        let mut machine = StateMachine::new();
        machine.apply(T::BuildStarted).expect("start");
        machine.fail(T::BuildFailed, "old error").expect("fail");
        machine.apply(T::BuildStarted).expect("rebuild");
        assert!(machine.failure().is_none(), "stale failure must be cleared");
    }

    #[test]
    fn a_failed_session_can_be_relaunched_after_a_reset() {
        let mut machine = machine_in(S::Failed);
        assert_eq!(machine.apply(T::Reset), Ok(S::Idle));
        assert_eq!(machine.apply(T::BuildStarted), Ok(S::Building));
    }

    #[test]
    fn a_finished_program_can_be_relaunched_without_rebuilding() {
        let mut machine = machine_in(S::Exited);
        assert!(machine.state().can_launch());
        assert_eq!(machine.apply(T::LaunchRequested), Ok(S::Starting));
    }

    #[test]
    fn the_program_can_exit_from_any_live_state() {
        for state in [S::Starting, S::Running, S::Paused] {
            let mut machine = machine_in(state);
            assert_eq!(machine.exited(Some(1)), Ok(S::Exited), "{state}");
        }
    }

    #[test]
    fn a_session_failure_is_accepted_only_while_a_session_exists() {
        for state in [S::Starting, S::Running, S::Paused] {
            let mut machine = machine_in(state);
            assert_eq!(machine.apply(T::SessionFailed), Ok(S::Failed), "{state}");
        }
        for state in [S::Idle, S::Ready, S::Building, S::Exited] {
            let mut machine = machine_in(state);
            assert!(machine.apply(T::SessionFailed).is_err(), "{state}");
        }
    }

    #[test]
    fn a_dead_debugger_can_always_be_recorded() {
        // The GDB process can die at any moment; the UI must never be left
        // claiming a session that no longer exists.
        for state in S::ALL {
            let mut machine = machine_in(state);
            machine.force_failed("gdb exited unexpectedly");
            assert_eq!(machine.state(), S::Failed, "from {state}");
            assert_eq!(machine.failure(), Some("gdb exited unexpectedly"));
        }
    }

    #[test]
    fn inspection_is_only_possible_while_paused() {
        for state in S::ALL {
            assert_eq!(
                state.can_inspect(),
                state == S::Paused,
                "{state} reported the wrong inspectability"
            );
        }
    }

    #[test]
    fn interruption_is_only_possible_while_running() {
        for state in S::ALL {
            assert_eq!(state.can_interrupt(), state == S::Running, "{state}");
        }
    }

    #[test]
    fn breakpoints_can_be_edited_except_while_the_program_is_moving() {
        assert!(S::Idle.can_edit_breakpoints(), "before a session");
        assert!(S::Paused.can_edit_breakpoints(), "while stopped");
        assert!(!S::Running.can_edit_breakpoints(), "while running");
        assert!(!S::Starting.can_edit_breakpoints(), "while starting");
    }

    #[test]
    fn allows_agrees_with_apply_for_every_pair() {
        // The query the UI uses to enable buttons must never disagree with
        // what actually happens.
        let transitions = [
            T::BuildStarted,
            T::BuildSucceeded,
            T::BuildFailed,
            T::LaunchRequested,
            T::LaunchSucceeded,
            T::LaunchFailed,
            T::Stopped,
            T::Resumed,
            T::ProgramExited,
            T::SessionFailed,
            T::Reset,
        ];
        for state in S::ALL {
            for transition in transitions {
                let machine = machine_in(state);
                let allowed = machine.allows(transition);
                let mut probe = machine_in(state);
                assert_eq!(
                    allowed,
                    probe.apply(transition).is_ok(),
                    "{state} + {transition:?} disagreed"
                );
            }
        }
    }

    #[test]
    fn every_state_is_reachable_from_idle() {
        // A state nothing can reach is dead code pretending to be a feature.
        let mut reached = vec![S::Idle];
        let transitions = [
            T::BuildStarted,
            T::BuildSucceeded,
            T::BuildFailed,
            T::LaunchRequested,
            T::LaunchSucceeded,
            T::LaunchFailed,
            T::Stopped,
            T::Resumed,
            T::ProgramExited,
            T::SessionFailed,
            T::Reset,
        ];

        let mut changed = true;
        while changed {
            changed = false;
            for state in reached.clone() {
                for transition in transitions {
                    if let Some(next) = next_state(state, transition) {
                        if !reached.contains(&next) {
                            reached.push(next);
                            changed = true;
                        }
                    }
                }
            }
        }

        for state in S::ALL {
            assert!(reached.contains(&state), "{state} is unreachable");
        }
    }

    #[test]
    fn state_labels_are_unique() {
        let mut labels: Vec<&str> = S::ALL.iter().map(|state| state.label()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count);
    }
}
