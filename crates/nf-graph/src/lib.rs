mod analysis;
mod builtins;
mod catalog;
mod collapse;
mod dynamic;
mod generics;
mod macros;
mod pin;
mod scope;
mod search;
mod text;
mod types;

pub use builtins::{COMMENT, EXEC_REROUTE, REROUTE};
pub use dynamic::{
    CASE_PREFIX, CASES_PROPERTY, COUNT_PROPERTY, LENGTH_PROPERTY, LOOPING_PROPERTY, SwitchCase, TIMELINE, TRACK_PREFIX, TRACKS_PROPERTY, TimelineTrack,
    expand_definition, node_definitions, switch_cases, timeline_length, timeline_looping, timeline_tracks,
};
pub use generics::{TYPE_PARAMETER, contains_type_parameter, infer_binding, substitute};
pub use collapse::{Collapse, CollapseError, CollapseFailure, CollapseRequest, CollapsedFunction, check_collapse, collapse_into_function, collapse_selection};
pub use search::{HitKind, SearchHit, SearchResult, Usages, find_usages, search_package};
pub use analysis::{Diagnostic, DiagnosticSeverity, analyze_graph};
pub use catalog::{
    ApiBinding, BindingKind, CallbackDefinition, CatalogPayload, DynamicPins, ClassInfo, ContextWarning, EnumInfo, NodeCatalog, NodeDefinition, NodeKind,
    Side, cast_definitions,
};
pub use scope::{
    CUSTOM_SETTING_PREFIX, custom_setting_definition, custom_setting_id,
    CLASS_SELF, FUNCTION_ENTRY, ProjectEnum, ProjectStruct, enum_switch_definition, struct_break_id, struct_definitions, struct_make_id, struct_set_id, event_definitions, event_handler_id, event_send_id, event_send_to_player_id, FUNCTION_RETURN, GraphScope, ProjectClass, ScopeData, build_scope, call_definition,
    call_node_id, class_event_node_id, entry_definition, method_node_id, project_classes, return_definition,
    spawn_node_id, validate_signature, validate_variables, variable_get_id, variable_set_id, EXEC_TYPE, LibraryFunction, LibraryPackage,
    MACRO_INPUTS, MACRO_OUTPUTS, exported_functions, library_definition, library_node_id, macro_definition, macro_node_id,
    required_libraries, validate_macro_signature, variable_definitions, DISPATCHER_BINDING, DISPATCHER_EVENT, dispatcher_definitions,
    dispatcher_member, exported_classes, interface_definitions, LibraryClass, interface_flag, validate_interface,
};
pub use macros::{MacroBody, MacroExpansion, MacroOrigin, MacroProblem, expand_macros, macro_of};
pub use pin::{PinDefinition, PinType};
pub use types::{TypeCategory, TypeSystem};
