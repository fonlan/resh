//! Deterministic, Tauri-free Agent loop planner/reducer used by the rollout test suite.
//!
//! This module deliberately models only lifecycle decisions: a real provider, database, SSH
//! transport, and Tauri event emission stay outside it. Keeping the reducer pure lets tests
//! exercise multi-turn ordering and terminal invariants without credentials or an application
//! runtime.

use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolKind {
    ReadOnly,
    RequiresApproval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FauxToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub kind: ToolKind,
}

impl FauxToolCall {
    pub(super) fn read_only(id: &str, name: &str, arguments: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            kind: ToolKind::ReadOnly,
        }
    }

    pub(super) fn requires_approval(id: &str, name: &str, arguments: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            kind: ToolKind::RequiresApproval,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FauxProviderTurn {
    Complete,
    Tools(Vec<FauxToolCall>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InvocationState {
    Executing,
    AwaitingApproval,
    Completed,
    Declined,
    Cancelled,
    Interrupted,
}

impl InvocationState {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Declined | Self::Cancelled | Self::Interrupted
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RunTerminalEvent {
    Completed,
    Cancelled,
    BudgetExceeded,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannerLimits {
    pub max_model_turns: u32,
    pub max_total_tools: u32,
    pub max_identical_tools: u32,
}

impl PlannerLimits {
    pub(super) const fn for_tests() -> Self {
        Self {
            max_model_turns: 12,
            max_total_tools: 128,
            max_identical_tools: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentLoopState {
    pub model_turns: u32,
    pub total_tools: u32,
    pub invocations: BTreeMap<String, InvocationState>,
    tool_signatures: BTreeMap<(String, String), u32>,
    pub terminal_events: Vec<RunTerminalEvent>,
    pub terminal_tool_outcomes: Vec<(String, InvocationState)>,
    awaiting_approval: Vec<String>,
    pending_execution: Vec<String>,
}

impl Default for AgentLoopState {
    fn default() -> Self {
        Self {
            model_turns: 0,
            total_tools: 0,
            invocations: BTreeMap::new(),
            tool_signatures: BTreeMap::new(),
            terminal_events: Vec::new(),
            terminal_tool_outcomes: Vec::new(),
            awaiting_approval: Vec::new(),
            pending_execution: Vec::new(),
        }
    }
}

impl AgentLoopState {
    pub(super) fn terminal_event(&self) -> Option<RunTerminalEvent> {
        self.terminal_events.first().copied()
    }

    pub(super) fn is_terminal(&self) -> bool {
        self.terminal_event().is_some()
    }

    pub(super) fn all_invocations_terminal_once(&self) -> bool {
        self.invocations.values().all(|status| status.is_terminal())
            && self.terminal_tool_outcomes.len() == self.invocations.len()
            && self.invocations.iter().all(|(id, status)| {
                self.terminal_tool_outcomes
                    .iter()
                    .filter(|(outcome_id, _)| outcome_id == id)
                    .all(|(_, outcome)| outcome == status)
                    && self
                        .terminal_tool_outcomes
                        .iter()
                        .filter(|(outcome_id, _)| outcome_id == id)
                        .count()
                        == 1
            })
    }

    fn finish(&mut self, event: RunTerminalEvent) {
        if self.terminal_events.is_empty() {
            self.terminal_events.push(event);
        }
    }

    fn terminalize_invocation(&mut self, id: &str, terminal: InvocationState) {
        debug_assert!(terminal.is_terminal());
        let should_terminalize = self
            .invocations
            .get(id)
            .is_some_and(|status| !status.is_terminal());
        if should_terminalize {
            self.invocations.insert(id.to_string(), terminal);
            self.terminal_tool_outcomes.push((id.to_string(), terminal));
        }
    }

    fn reserve_model_turn(&mut self, limits: &PlannerLimits) -> bool {
        if self.model_turns >= limits.max_model_turns {
            self.finish(RunTerminalEvent::BudgetExceeded);
            return false;
        }
        self.model_turns += 1;
        true
    }

    fn plan_tools(&mut self, calls: &[FauxToolCall], limits: &PlannerLimits) -> bool {
        if self.total_tools.saturating_add(calls.len() as u32) > limits.max_total_tools {
            self.finish(RunTerminalEvent::BudgetExceeded);
            return false;
        }

        let mut incoming_ids = BTreeMap::<&str, ()>::new();
        let mut incoming_signatures = BTreeMap::<(String, String), u32>::new();
        for call in calls {
            if self.invocations.contains_key(&call.id)
                || incoming_ids.insert(call.id.as_str(), ()).is_some()
            {
                self.finish(RunTerminalEvent::BudgetExceeded);
                return false;
            }
            *incoming_signatures
                .entry((call.name.clone(), call.arguments.clone()))
                .or_default() += 1;
        }
        for (signature, incoming) in &incoming_signatures {
            let prior = self
                .tool_signatures
                .get(signature)
                .copied()
                .unwrap_or_default();
            if prior.saturating_add(*incoming) > limits.max_identical_tools {
                self.finish(RunTerminalEvent::BudgetExceeded);
                return false;
            }
        }

        self.total_tools += calls.len() as u32;
        for call in calls {
            let status = match call.kind {
                ToolKind::ReadOnly => InvocationState::Executing,
                ToolKind::RequiresApproval => {
                    self.awaiting_approval.push(call.id.clone());
                    InvocationState::AwaitingApproval
                }
            };
            *self
                .tool_signatures
                .entry((call.name.clone(), call.arguments.clone()))
                .or_default() += 1;
            self.invocations.insert(call.id.clone(), status);
        }
        true
    }

    fn complete_read_only_batch(&mut self) {
        let ids = self
            .invocations
            .iter()
            .filter_map(|(id, status)| (*status == InvocationState::Executing).then(|| id.clone()))
            .collect::<Vec<_>>();
        for id in ids {
            self.terminalize_invocation(&id, InvocationState::Completed);
        }
    }

    fn complete_approved_execution_batch(&mut self) {
        let ids = std::mem::take(&mut self.pending_execution);
        for id in ids {
            self.terminalize_invocation(&id, InvocationState::Completed);
        }
    }

    pub(super) fn approve_pending(&mut self) {
        if self.is_terminal() {
            return;
        }
        for id in self.awaiting_approval.drain(..) {
            if self
                .invocations
                .get(&id)
                .is_some_and(|status| *status == InvocationState::AwaitingApproval)
            {
                self.invocations
                    .insert(id.clone(), InvocationState::Executing);
                self.pending_execution.push(id);
            }
        }
    }

    pub(super) fn decline_pending(&mut self) {
        if self.is_terminal() {
            return;
        }
        let pending = std::mem::take(&mut self.awaiting_approval);
        for id in pending {
            self.terminalize_invocation(&id, InvocationState::Declined);
        }
    }

    pub(super) fn cancel(&mut self) {
        if self.is_terminal() {
            return;
        }
        let ids = self
            .invocations
            .iter()
            .filter_map(|(id, status)| (!status.is_terminal()).then(|| id.clone()))
            .collect::<Vec<_>>();
        for id in ids {
            self.terminalize_invocation(&id, InvocationState::Cancelled);
        }
        self.awaiting_approval.clear();
        self.pending_execution.clear();
        self.finish(RunTerminalEvent::Cancelled);
    }

    pub(super) fn recover_after_crash(&mut self) {
        if self.is_terminal() {
            return;
        }
        let ids = self
            .invocations
            .iter()
            .filter_map(|(id, status)| (!status.is_terminal()).then(|| id.clone()))
            .collect::<Vec<_>>();
        for id in ids {
            self.terminalize_invocation(&id, InvocationState::Interrupted);
        }
        self.awaiting_approval.clear();
        self.pending_execution.clear();
        self.finish(RunTerminalEvent::Interrupted);
    }
}

/// A scripted provider keeps the tests deterministic: every request consumes one predefined
/// turn and never performs I/O.
#[derive(Debug, Clone)]
pub(super) struct ScriptedFauxProvider {
    turns: VecDeque<FauxProviderTurn>,
}

impl ScriptedFauxProvider {
    pub(super) fn new(turns: impl IntoIterator<Item = FauxProviderTurn>) -> Self {
        Self {
            turns: turns.into_iter().collect(),
        }
    }

    fn next_turn(&mut self) -> Option<FauxProviderTurn> {
        self.turns.pop_front()
    }
}

/// Drive automatic read-only calls and provider turns until the provider completes, the run is
/// terminal, or a mutating call needs an explicit approval action.
pub(super) fn drive_until_blocked(
    state: &mut AgentLoopState,
    provider: &mut ScriptedFauxProvider,
    limits: &PlannerLimits,
) {
    while !state.is_terminal() {
        state.complete_approved_execution_batch();
        if !state.awaiting_approval.is_empty() {
            return;
        }
        let Some(turn) = provider.next_turn() else {
            state.finish(RunTerminalEvent::Completed);
            return;
        };
        if !state.reserve_model_turn(limits) {
            return;
        }
        match turn {
            FauxProviderTurn::Complete => {
                state.finish(RunTerminalEvent::Completed);
                return;
            }
            FauxProviderTurn::Tools(calls) => {
                if !state.plan_tools(&calls, limits) {
                    return;
                }
                state.complete_read_only_batch();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        drive_until_blocked, AgentLoopState, FauxProviderTurn, FauxToolCall, InvocationState,
        PlannerLimits, RunTerminalEvent, ScriptedFauxProvider,
    };

    fn read(id: &str, arguments: &str) -> FauxToolCall {
        FauxToolCall::read_only(id, "read_file", arguments)
    }

    fn command(id: &str) -> FauxToolCall {
        FauxToolCall::requires_approval(id, "run_in_terminal", r#"{"command":"pwd"}"#)
    }

    fn assert_single_terminal(state: &AgentLoopState, expected: RunTerminalEvent) {
        assert_eq!(state.terminal_events, vec![expected]);
        assert!(state.all_invocations_terminal_once());
    }

    #[test]
    fn scripted_provider_natural_completion_emits_one_terminal_event() {
        let mut state = AgentLoopState::default();
        let mut provider = ScriptedFauxProvider::new([FauxProviderTurn::Complete]);
        drive_until_blocked(&mut state, &mut provider, &PlannerLimits::for_tests());

        assert_single_terminal(&state, RunTerminalEvent::Completed);
        assert_eq!(state.model_turns, 1);
    }

    #[test]
    fn continuous_read_only_batches_are_barriered_and_complete() {
        let mut state = AgentLoopState::default();
        let mut provider = ScriptedFauxProvider::new([
            FauxProviderTurn::Tools(vec![read("read-1", r#"{"remote_path":"/a"}"#)]),
            FauxProviderTurn::Tools(vec![read("read-2", r#"{"remote_path":"/b"}"#)]),
            FauxProviderTurn::Complete,
        ]);
        drive_until_blocked(&mut state, &mut provider, &PlannerLimits::for_tests());

        assert_single_terminal(&state, RunTerminalEvent::Completed);
        assert_eq!(state.model_turns, 3);
        assert_eq!(state.total_tools, 2);
        assert!(state
            .invocations
            .values()
            .all(|status| *status == InvocationState::Completed));
    }

    #[test]
    fn mixed_batch_waits_for_approval_before_the_next_provider_turn() {
        let mut state = AgentLoopState::default();
        let mut provider = ScriptedFauxProvider::new([
            FauxProviderTurn::Tools(vec![
                read("read-1", r#"{"remote_path":"/a"}"#),
                command("command-1"),
            ]),
            FauxProviderTurn::Complete,
        ]);
        let limits = PlannerLimits::for_tests();

        drive_until_blocked(&mut state, &mut provider, &limits);
        assert_eq!(state.model_turns, 1);
        assert!(state
            .invocations
            .values()
            .any(|status| *status == InvocationState::AwaitingApproval));
        assert!(state
            .invocations
            .values()
            .any(|status| *status == InvocationState::Completed));

        state.approve_pending();
        drive_until_blocked(&mut state, &mut provider, &limits);
        assert_single_terminal(&state, RunTerminalEvent::Completed);
        assert_eq!(state.model_turns, 2);
    }

    #[test]
    fn declined_tool_has_a_terminal_outcome_and_the_run_can_continue() {
        let mut state = AgentLoopState::default();
        let mut provider = ScriptedFauxProvider::new([
            FauxProviderTurn::Tools(vec![command("command-1")]),
            FauxProviderTurn::Complete,
        ]);
        let limits = PlannerLimits::for_tests();

        drive_until_blocked(&mut state, &mut provider, &limits);
        state.decline_pending();
        drive_until_blocked(&mut state, &mut provider, &limits);

        assert_single_terminal(&state, RunTerminalEvent::Completed);
        assert!(state
            .invocations
            .values()
            .all(|status| *status == InvocationState::Declined));
    }

    #[test]
    fn cancellation_terminalizes_pending_calls_without_a_second_terminal_event() {
        let mut state = AgentLoopState::default();
        let mut provider =
            ScriptedFauxProvider::new([FauxProviderTurn::Tools(vec![command("command-1")])]);
        drive_until_blocked(&mut state, &mut provider, &PlannerLimits::for_tests());

        state.cancel();
        state.cancel();

        assert_single_terminal(&state, RunTerminalEvent::Cancelled);
        assert!(state
            .invocations
            .values()
            .all(|status| *status == InvocationState::Cancelled));
    }

    #[test]
    fn budget_exhaustion_prevents_the_next_model_turn() {
        let mut state = AgentLoopState::default();
        let mut provider = ScriptedFauxProvider::new([
            FauxProviderTurn::Tools(vec![read("read-1", r#"{"remote_path":"/a"}"#)]),
            FauxProviderTurn::Complete,
        ]);
        let limits = PlannerLimits {
            max_model_turns: 1,
            ..PlannerLimits::for_tests()
        };
        drive_until_blocked(&mut state, &mut provider, &limits);

        assert_single_terminal(&state, RunTerminalEvent::BudgetExceeded);
        assert_eq!(state.model_turns, 1);
    }

    #[test]
    fn identical_tool_arguments_are_budget_limited() {
        let mut state = AgentLoopState::default();
        let mut provider = ScriptedFauxProvider::new([
            FauxProviderTurn::Tools(vec![read("read-1", r#"{"remote_path":"/same"}"#)]),
            FauxProviderTurn::Tools(vec![read("read-2", r#"{"remote_path":"/same"}"#)]),
            FauxProviderTurn::Tools(vec![read("read-3", r#"{"remote_path":"/same"}"#)]),
            FauxProviderTurn::Tools(vec![read("read-4", r#"{"remote_path":"/same"}"#)]),
        ]);
        drive_until_blocked(&mut state, &mut provider, &PlannerLimits::for_tests());

        assert_single_terminal(&state, RunTerminalEvent::BudgetExceeded);
        assert_eq!(state.total_tools, 3);
    }

    #[test]
    fn duplicate_tool_ids_are_rejected_without_creating_terminal_outcomes() {
        let mut state = AgentLoopState::default();
        let mut provider = ScriptedFauxProvider::new([FauxProviderTurn::Tools(vec![
            read("duplicate", r#"{"remote_path":"/a"}"#),
            read("duplicate", r#"{"remote_path":"/b"}"#),
        ])]);

        drive_until_blocked(&mut state, &mut provider, &PlannerLimits::for_tests());

        assert_single_terminal(&state, RunTerminalEvent::BudgetExceeded);
        assert!(state.invocations.is_empty());
        assert!(state.terminal_tool_outcomes.is_empty());
    }

    #[test]
    fn crash_recovery_interrupts_an_executing_side_effect_without_replay() {
        let mut state = AgentLoopState::default();
        let mut provider = ScriptedFauxProvider::new([
            FauxProviderTurn::Tools(vec![command("command-1")]),
            FauxProviderTurn::Complete,
        ]);
        let limits = PlannerLimits::for_tests();
        drive_until_blocked(&mut state, &mut provider, &limits);

        state.approve_pending();
        assert_eq!(
            state.invocations.get("command-1"),
            Some(&InvocationState::Executing),
            "the reducer must model a side effect that has started but not reached a terminal outcome"
        );
        state.recover_after_crash();
        drive_until_blocked(&mut state, &mut provider, &limits);

        assert_single_terminal(&state, RunTerminalEvent::Interrupted);
        assert_eq!(
            state.invocations.get("command-1"),
            Some(&InvocationState::Interrupted)
        );
        assert_eq!(
            state.model_turns, 1,
            "recovery must not fetch another provider turn or replay the side effect"
        );
    }
}
