use std::collections::{BTreeSet, HashSet};

use serde::Serialize;

use super::model::{
    CodingState, CompletedItem, PlannedTask, RunStatus, RuntimeError, StepEnvelope, StepOutcome,
    WorkItem, WorkPlan, MAX_PACKET_TASKS, MAX_TASK_SCOPE_PATHS, MAX_WORK_PLAN_TASKS,
    RUNTIME_SCHEMA_VERSION,
};
use super::validate::{validate_relative_path, validate_safe_id, validate_text, MAX_ITEM_BYTES};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PacketPreview {
    pub schema_version: u64,
    pub run_name: String,
    pub revision: u64,
    pub selected_task_ids: Vec<String>,
    pub tasks: Vec<PlannedTask>,
    pub reason: String,
    pub remaining_count: usize,
}

pub fn validate_work_plan(plan: &WorkPlan) -> Result<(), RuntimeError> {
    if plan.schema_version != RUNTIME_SCHEMA_VERSION {
        return Err(RuntimeError::UnsupportedSchema {
            expected: RUNTIME_SCHEMA_VERSION,
            actual: plan.schema_version,
        });
    }
    if !(1..=MAX_PACKET_TASKS).contains(&plan.max_packet_tasks) {
        return Err(RuntimeError::InvalidManifest(format!(
            "max_packet_tasks must be between 1 and {MAX_PACKET_TASKS}"
        )));
    }
    if plan.tasks.is_empty() || plan.tasks.len() > MAX_WORK_PLAN_TASKS {
        return Err(RuntimeError::InvalidManifest(format!(
            "work plan tasks must contain between 1 and {MAX_WORK_PLAN_TASKS} items"
        )));
    }

    let mut ids = HashSet::new();
    for task in &plan.tasks {
        validate_safe_id(&task.id)?;
        validate_safe_id(&task.group)?;
        validate_text("work_plan.task", &task.task, MAX_ITEM_BYTES, false)?;
        if !ids.insert(task.id.as_str()) {
            return Err(RuntimeError::DuplicateId(task.id.clone()));
        }
        if task.scope.is_empty() || task.scope.len() > MAX_TASK_SCOPE_PATHS {
            return Err(RuntimeError::InvalidManifest(format!(
                "task {} scope must contain between 1 and {MAX_TASK_SCOPE_PATHS} paths",
                task.id
            )));
        }
        let mut paths = HashSet::new();
        for path in &task.scope {
            validate_relative_path(path)?;
            if !paths.insert(path) {
                return Err(RuntimeError::InvalidState(format!(
                    "duplicate scope path: {path}"
                )));
            }
        }
        let mut dependencies = HashSet::new();
        for dependency in &task.depends_on {
            validate_safe_id(dependency)?;
            if !dependencies.insert(dependency) {
                return Err(RuntimeError::DuplicateId(dependency.clone()));
            }
        }
    }
    for task in &plan.tasks {
        for dependency in &task.depends_on {
            if !ids.contains(dependency.as_str()) {
                return Err(RuntimeError::InvalidManifest(format!(
                    "task {} has unknown dependency {dependency}",
                    task.id
                )));
            }
        }
    }
    reject_cycles(plan)?;
    Ok(())
}

fn reject_cycles(plan: &WorkPlan) -> Result<(), RuntimeError> {
    let mut completed = HashSet::new();
    loop {
        let before = completed.len();
        for task in &plan.tasks {
            if !completed.contains(task.id.as_str())
                && task
                    .depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency.as_str()))
            {
                completed.insert(task.id.as_str());
            }
        }
        if completed.len() == plan.tasks.len() {
            return Ok(());
        }
        if completed.len() == before {
            return Err(RuntimeError::InvalidManifest(
                "work plan dependencies contain a cycle".into(),
            ));
        }
    }
}

pub fn validate_plan_state(plan: &WorkPlan, state: &CodingState) -> Result<(), RuntimeError> {
    validate_work_plan(plan)?;
    let declared: HashSet<_> = plan.tasks.iter().map(|task| task.id.as_str()).collect();
    let completed: HashSet<_> = state
        .completed
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    if completed.iter().any(|id| !declared.contains(id)) {
        return Err(RuntimeError::InvalidState(
            "completed task is not declared by the work plan".into(),
        ));
    }
    let expected = remaining_queue(plan, &completed);
    if state.queue != expected {
        return Err(RuntimeError::InvalidState(
            "work queue differs from the immutable work plan".into(),
        ));
    }
    if state.status == RunStatus::Complete && completed.len() != plan.tasks.len() {
        return Err(RuntimeError::IncompleteQueue);
    }
    Ok(())
}

pub fn initial_queue(plan: &WorkPlan) -> Vec<WorkItem> {
    plan.tasks
        .iter()
        .map(|task| WorkItem {
            id: task.id.clone(),
            task: task.task.clone(),
        })
        .collect()
}

pub fn select_tasks<'a>(
    plan: &'a WorkPlan,
    state: &CodingState,
) -> Result<Vec<&'a PlannedTask>, RuntimeError> {
    validate_plan_state(plan, state)?;
    if state.status != RunStatus::Running {
        return Err(RuntimeError::TerminalRun);
    }
    let completed: HashSet<_> = state
        .completed
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let first = plan
        .tasks
        .iter()
        .find(|task| {
            !completed.contains(task.id.as_str())
                && task
                    .depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency.as_str()))
        })
        .ok_or_else(|| RuntimeError::InvalidState("no ready work-plan task".into()))?;
    let first_scope: BTreeSet<_> = first.scope.iter().collect();
    let mut selected = vec![first];
    while selected.len() < plan.max_packet_tasks {
        let selected_ids: HashSet<_> = selected.iter().map(|task| task.id.as_str()).collect();
        let next = plan.tasks.iter().find(|task| {
            !completed.contains(task.id.as_str())
                && !selected_ids.contains(task.id.as_str())
                && task.group == first.group
                && task.risk == first.risk
                && task.scope.iter().collect::<BTreeSet<_>>() == first_scope
                && task.depends_on.iter().all(|dependency| {
                    completed.contains(dependency.as_str())
                        || selected_ids.contains(dependency.as_str())
                })
        });
        match next {
            Some(task) => selected.push(task),
            None => break,
        }
    }
    Ok(selected)
}

pub fn preview(
    run_name: &str,
    plan: &WorkPlan,
    state: &CodingState,
) -> Result<PacketPreview, RuntimeError> {
    let tasks = select_tasks(plan, state)?;
    Ok(PacketPreview {
        schema_version: RUNTIME_SCHEMA_VERSION,
        run_name: run_name.into(),
        revision: state.revision,
        selected_task_ids: tasks.iter().map(|task| task.id.clone()).collect(),
        tasks: tasks.into_iter().cloned().collect(),
        reason: "first remaining ready task plus declaration-order ready tasks with identical group, risk, and scope set".into(),
        remaining_count: state.queue.len(),
    })
}

pub fn validate_and_derive_delta(
    plan: &WorkPlan,
    current: &CodingState,
    mut envelope: StepEnvelope,
    verification_passed: bool,
) -> Result<StepEnvelope, RuntimeError> {
    let packet = select_tasks(plan, current)?;
    if !verification_passed {
        return Err(RuntimeError::VerificationRequired);
    }
    let completed = &envelope.delta.completed_add;
    if envelope.outcome == StepOutcome::Blocked {
        if !completed.is_empty() {
            return Err(RuntimeError::InvalidState(
                "blocked packet cannot accept completed tasks".into(),
            ));
        }
    } else {
        if completed.is_empty() {
            return Err(RuntimeError::InvalidState(
                "managed packet must accept a nonempty prefix".into(),
            ));
        }
        if completed.len() > packet.len()
            || completed
                .iter()
                .zip(packet.iter())
                .any(|(actual, expected)| actual.id != expected.id)
        {
            return Err(RuntimeError::InvalidState(
                "completed tasks must be a prefix of the selected packet".into(),
            ));
        }
    }
    let prior: HashSet<_> = current
        .completed
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let accepted: HashSet<_> = completed.iter().map(|item| item.id.as_str()).collect();
    if accepted.len() != completed.len() || accepted.iter().any(|id| prior.contains(id)) {
        return Err(RuntimeError::InvalidState(
            "completed tasks contain duplicate or previously accepted ids".into(),
        ));
    }
    let all_completed: HashSet<_> = prior.union(&accepted).copied().collect();
    let expected_queue = remaining_queue(plan, &all_completed);
    if let Some(proposed) = envelope.delta.queue_replace.as_ref() {
        if proposed != &expected_queue {
            return Err(RuntimeError::InvalidState(
                "queue replacement conflicts with the immutable work plan".into(),
            ));
        }
    }
    if envelope.outcome == StepOutcome::Complete && !expected_queue.is_empty() {
        return Err(RuntimeError::IncompleteQueue);
    }
    envelope.delta.queue_replace = Some(expected_queue);
    Ok(envelope)
}

pub fn accepted_ids(envelope: &StepEnvelope) -> Vec<String> {
    envelope
        .delta
        .completed_add
        .iter()
        .map(|item: &CompletedItem| item.id.clone())
        .collect()
}

fn remaining_queue(plan: &WorkPlan, completed: &HashSet<&str>) -> Vec<WorkItem> {
    plan.tasks
        .iter()
        .filter(|task| !completed.contains(task.id.as_str()))
        .map(|task| WorkItem {
            id: task.id.clone(),
            task: task.task.clone(),
        })
        .collect()
}
