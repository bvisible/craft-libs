use craft_ai_core::role::{eligible, fits, resolve};
use craft_ai_core::*;

fn draft(endpoint: &str) -> ConnectionDraft {
    let mut draft = ConnectionDraft::default();
    draft.label = "Test".into();
    draft.endpoint = endpoint.into();
    draft.uses_key = false;
    draft
}

fn models(id: &str) -> Vec<ModelInfo> {
    vec![ModelInfo::new(ModelId::new(id).unwrap(), id)]
}

#[test]
fn new_draft_discards_models_when_address_changes_after_probe() {
    let mut draft = draft("https://a.example/v1");
    draft.set_models(models("a-only"));
    draft.endpoint = "https://b.example/v1".into();
    assert!(draft.prepare(&[]).unwrap().connection.models.is_empty());
}

#[test]
fn returning_to_original_address_does_not_keep_another_addresses_models() {
    let saved = draft("https://a.example/v1").prepare(&[]).unwrap().connection;
    let mut draft = ConnectionDraft::from_connection(&saved);
    draft.endpoint = "https://b.example/v1".into();
    draft.set_models(models("b-only"));
    draft.endpoint = "https://a.example/v1".into();
    assert!(draft.prepare(&[saved]).unwrap().connection.models.is_empty());
}

#[test]
fn changing_model_contents_does_not_establish_address_provenance() {
    let saved = draft("https://a.example/v1").prepare(&[]).unwrap().connection;
    let mut draft = ConnectionDraft::from_connection(&saved);
    draft.models = models("refreshed-at-a");
    draft.endpoint = "https://b.example/v1".into();
    assert!(draft.prepare(&[saved]).unwrap().connection.models.is_empty());
}

#[test]
fn preset_models_belong_to_the_preset_address() {
    let preset = PRESETS.iter().find(|preset| preset.key == "vertex-express").unwrap();
    let mut draft = ConnectionDraft::from_preset(preset);
    draft.key = "test-key".into();
    assert!(!draft.prepare(&[]).unwrap().connection.models.is_empty());
    draft.endpoint = "https://other.example/v1".into();
    assert!(draft.prepare(&[]).unwrap().connection.models.is_empty());
}

#[test]
fn a_fresh_test_at_each_address_keeps_its_models() {
    let saved = draft("https://a.example/v1").prepare(&[]).unwrap().connection;
    let mut draft = ConnectionDraft::from_connection(&saved);
    for address in ["https://b.example/v1", "https://a.example/v1", "https://c.example/v1"] {
        draft.endpoint = format!("  {address}  ");
        draft.set_models(models("same-model"));
        assert_eq!(draft.prepare(std::slice::from_ref(&saved)).unwrap().connection.models, models("same-model"));
    }
}

#[test]
fn editing_and_returning_without_a_probe_preserves_original_models() {
    let mut saved = draft("https://a.example/v1").prepare(&[]).unwrap().connection;
    saved.models = models("a-only");
    let mut draft = ConnectionDraft::from_connection(&saved);
    draft.endpoint = "https://b.example/v1".into();
    assert!(draft.prepare(std::slice::from_ref(&saved)).unwrap().connection.models.is_empty());
    draft.endpoint = "https://a.example/v1".into();
    assert_eq!(draft.prepare(std::slice::from_ref(&saved)).unwrap().connection.models, saved.models);
}

#[test]
fn custom_manual_models_still_work_for_a_new_connection_and_unchanged_address() {
    let mut draft = ConnectionDraft::custom();
    draft.endpoint = "https://custom.example/v1".into();
    draft.uses_key = false;
    draft.models = models("manual-model");
    let saved = draft.prepare(&[]).unwrap().connection;
    assert_eq!(saved.models, models("manual-model"));
    let mut edited = ConnectionDraft::from_connection(&saved);
    edited.models = models("other-manual-model");
    assert_eq!(edited.prepare(&[saved]).unwrap().connection.models, models("other-manual-model"));
}

fn role_and_connection() -> (Role, Connection, Binding) {
    let capability = CapabilityId::parse("image.inpaint").unwrap();
    let role = Role {
        id: RoleId::new("source-only").unwrap(),
        label: "Source only".into(),
        capability: capability.clone(),
        inputs: vec![InputRole::Source],
    };
    let mut connection = draft("https://a.example/v1").prepare(&[]).unwrap().connection;
    connection.models = vec![
        ModelInfo::new(ModelId::new("needs-mask").unwrap(), "Needs mask").with(
            CapabilitySpec::new(capability)
                .accepting(InputRole::Source, 1)
                .accepting(InputRole::Mask, 1)
                .requiring(InputRole::Source)
                .requiring(InputRole::Mask),
        ),
    ];
    let binding = Binding { role: role.id.clone(), connection: connection.id.clone(), model: connection.models[0].id.clone() };
    (role, connection, binding)
}

#[test]
fn model_picker_excludes_inputs_the_tool_cannot_supply() {
    let (role, connection, _) = role_and_connection();
    let request = Request::new(role.capability.clone()).with_input(InputRole::Source, Payload::inline("image/png", vec![1]).unwrap());
    assert!(request.validate(&connection.models[0].capabilities[0], &Limits::default()).is_err());
    assert!(fits(&role, &connection.models[0]).is_none());
    assert!(eligible(&role, &[connection]).is_empty());
}

#[test]
fn binding_resolution_rejects_inputs_the_tool_cannot_supply() {
    let (role, connection, binding) = role_and_connection();
    assert!(resolve(&role, &[&[binding]], &[connection]).is_err());
}

#[test]
fn required_inputs_resolve_when_supplied_and_optional_inputs_need_not_be_sent() {
    let (mut role, mut connection, binding) = role_and_connection();
    role.inputs.push(InputRole::Mask);
    assert!(fits(&role, &connection.models[0]).is_some());
    assert!(resolve(&role, &[std::slice::from_ref(&binding)], std::slice::from_ref(&connection)).is_ok());
    role.inputs = vec![InputRole::Source];
    connection.models[0].capabilities[0].requires = vec![InputRole::Source];
    assert!(fits(&role, &connection.models[0]).is_some());
    assert!(resolve(&role, &[&[binding]], &[connection]).is_ok());
}
