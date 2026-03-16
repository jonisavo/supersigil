//! Tests for the graph module.

pub(crate) mod generators;

mod prop_component_index;
mod prop_cycle;
mod prop_document_index;
mod prop_error_aggregation;
mod prop_ref_resolution;
mod prop_reverse;
mod prop_task_implements;
mod prop_topo;
mod prop_tracked_files;

mod prop_context;
mod prop_context_decisions;
mod prop_context_linked_decisions;
mod prop_plan;

mod prop_decision_integration;

mod unit;
