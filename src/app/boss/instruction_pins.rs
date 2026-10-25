//! The source-backed, revision-aware instruction list and human editor.
use super::*;
use waku_protocol::instruction_pins::{
    InstructionPin, InstructionPinOperation as Action, InstructionPinRetirement,
    InstructionPinState,
};

pub(super) struct InstructionPinEditor {
    pub(super) key: DaemonKey,
    pub(super) id: Option<Uuid>,
    pub(super) revision: u64,
    pub(super) instruction: Entity<TextInput>,
    pub(super) condition: Entity<TextInput>,
    pub(super) project_id: Option<Uuid>,
    pub(super) retire_on: Option<InstructionPinRetirement>,
    automatic_condition: Option<String>,
    project_label: Option<String>,
    outcome_label: Option<String>,
    projects: Arc<Vec<(Uuid, String)>>,
    outcomes: Arc<Vec<(Uuid, String)>>,
    source: Option<(String, bool)>,
}

impl Waku {
    fn edit_instruction_pin(
        &mut self,
        key: DaemonKey,
        pin: Option<InstructionPin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.boss_ui.pin_editor.is_some() {
            self.show_toast(tr!("boss.instruction_finish_edit"));
            return;
        }
        let mut input = |label: String, value: String, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(window, cx)
                    .tab_index(0)
                    .accessibility_label(label)
                    .multi_line()
                    .auto_height()
                    .max_lines(6);
                input.set_content(value, cx);
                input
            })
        };
        let instruction = input(
            tr!("boss.instruction_text"),
            pin.as_ref()
                .map(|pin| pin.instruction.clone())
                .unwrap_or_default(),
            cx,
        );
        let condition = input(
            tr!("boss.instruction_condition"),
            pin.as_ref()
                .map(|pin| pin.retire_when.clone())
                .unwrap_or_default(),
            cx,
        );
        cx.subscribe(&condition, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Edited) {
                if let Some(editor) = &mut this.boss_ui.pin_editor {
                    if editor.condition.entity_id() == input.entity_id()
                        && editor.retire_on.is_some()
                        && editor.automatic_condition.as_deref() != Some(input.read(cx).content())
                    {
                        // A compound or edited condition requires judgment;
                        // completion of one outcome no longer proves release.
                        editor.retire_on = None;
                        editor.automatic_condition = None;
                        editor.outcome_label = None;
                    }
                }
                cx.notify();
            }
        })
        .detach();
        let projects: Arc<Vec<(Uuid, String)>> = Arc::new(
            self.state
                .projects
                .iter()
                .filter(|project| {
                    self.daemons.project_owner(project.id) == key
                        && self
                            .boss_ui
                            .states
                            .get(&key)
                            .is_none_or(|boss| boss.identity.id != project.id)
                })
                .map(|project| (project.id, project.name.clone()))
                .collect(),
        );
        let outcomes: Arc<Vec<(Uuid, String)>> = Arc::new(
            self.boss_ui
                .states
                .get(&key)
                .into_iter()
                .flat_map(|state| state.outcomes.iter())
                .filter(|outcome| !outcome.terminal())
                .map(|outcome| (outcome.id, outcome.outcome.clone()))
                .collect(),
        );
        let project_label = pin.as_ref().and_then(|pin| pin.project_id).and_then(|id| {
            projects
                .iter()
                .find(|(project, _)| *project == id)
                .map(|(_, label)| label.clone())
        });
        let outcome_label = pin.as_ref().and_then(|pin| pin.retire_on).and_then(
            |InstructionPinRetirement::OutcomeCompleted { outcome_id }| {
                outcomes
                    .iter()
                    .find(|(id, _)| *id == outcome_id)
                    .map(|(_, label)| label.clone())
            },
        );
        self.boss_ui.pin_editor = Some(InstructionPinEditor {
            key,
            id: pin.as_ref().map(|pin| pin.id),
            revision: pin.as_ref().map_or(0, |pin| pin.revision),
            instruction: instruction.clone(),
            condition,
            project_id: pin.as_ref().and_then(|pin| pin.project_id),
            retire_on: pin.as_ref().and_then(|pin| pin.retire_on),
            automatic_condition: pin
                .as_ref()
                .filter(|pin| pin.retire_on.is_some())
                .map(|pin| pin.retire_when.clone()),
            project_label,
            outcome_label,
            projects,
            outcomes,
            source: pin.and_then(|pin| {
                pin.source
                    .map(|source| (source.quote, source.quote_truncated))
            }),
        });
        window.focus(&instruction.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    fn save_instruction_pin(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.boss_ui.pin_editor else {
            return;
        };
        let key = editor.key;
        if self.boss_ui.pending {
            self.show_toast(tr!("boss.instruction_saving"));
            return;
        }
        let instruction = editor.instruction.read(cx).content().to_owned();
        let retire_when = editor.condition.read(cx).content().to_owned();
        let action = match editor.id {
            Some(id) => Action::Edit {
                id,
                expected_revision: editor.revision,
                instruction,
                retire_when,
                project_id: editor.project_id,
                retire_on: editor.retire_on,
            },
            None => Action::Pin {
                instruction,
                retire_when,
                project_id: editor.project_id,
                retire_on: editor.retire_on,
                source_session_id: None,
                source_message_id: None,
            },
        };
        self.boss_request(
            key,
            BossOperation::InstructionPin { action },
            BossReply::InstructionsSaved,
            cx,
        );
    }

    pub(super) fn render_instruction_pins(
        &self,
        key: DaemonKey,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let history = self.boss_ui.pin_history.contains(&key);
        let filter = self.boss_segmented(
            "instruction-history",
            vec![
                (false, tr!("boss.instructions_active")),
                (true, tr!("boss.instructions_history")),
            ],
            history,
            cx,
            move |this, picked, _, cx| {
                if picked {
                    this.boss_ui.pin_history.insert(key);
                } else {
                    this.boss_ui.pin_history.remove(&key);
                }
                this.sync_boss_page_rows();
                cx.notify();
            },
        );
        let mut page = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px(px(20.0))
                    .py(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(filter)
                    .child(div().flex_1())
                    .child(
                        boss_button("instruction-new", tr!("boss.instruction_new"), &theme)
                            .child(tr!("boss.instruction_new"))
                            .on_activation(cx, move |this, window, cx| {
                                this.edit_instruction_pin(key, None, window, cx)
                            }),
                    ),
            )
            .child(
                div()
                    .px(px(20.0))
                    .pb(px(8.0))
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("boss.instructions_hint")),
            );
        if self
            .boss_ui
            .pin_editor
            .as_ref()
            .is_some_and(|editor| editor.key == key)
        {
            page = page.child(self.render_instruction_pin_editor(cx));
        }
        if self.boss_ui.rows.is_empty() {
            page = page.child(boss_empty_state(
                &theme,
                "icons/file-text.svg",
                tr!("boss.instructions_empty"),
                tr!("boss.instructions_empty_hint"),
            ));
        } else {
            page = page.child(self.boss_item_list(cx));
        }
        page.into_any_element()
    }

    pub(super) fn render_instruction_pin_row(
        &self,
        key: DaemonKey,
        id: Uuid,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(pin) = self.boss_ui.pin_rows.get(&(key, id)).cloned() else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let label = match pin.state {
            InstructionPinState::Candidate => tr!("boss.instruction_candidate"),
            InstructionPinState::Active => tr!("boss.instruction_active"),
            InstructionPinState::Retired => tr!("boss.instruction_retired"),
            InstructionPinState::Superseded => tr!("boss.instruction_superseded"),
        };
        let live = matches!(
            pin.state,
            InstructionPinState::Candidate | InstructionPinState::Active
        );
        let mut row = div()
            .px(px(20.0))
            .py(px(12.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .border_b_1()
            .border_color(theme.separator)
            .child(
                div()
                    .text_size(sp(11.0))
                    .text_color(theme.text_secondary)
                    .child(label),
            )
            .child(
                div()
                    .text_size(sp(13.0))
                    .text_color(theme.text)
                    .child(pin.instruction.clone()),
            );
        if !pin.retire_when.is_empty() {
            row = row.child(
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(format!(
                        "{} {}",
                        tr!("boss.instruction_condition"),
                        pin.retire_when
                    )),
            );
        }
        let scope = self
            .boss_ui
            .pin_scope_labels
            .get(&(key, id))
            .cloned()
            .unwrap_or_else(|| tr!("boss.instruction_all_projects"));
        row = row.child(
            div()
                .text_size(sp(11.0))
                .text_color(theme.text_tertiary)
                .child(format!("{} {scope}", tr!("boss.instruction_scope"))),
        );
        if let Some(resolution) = &pin.resolution {
            row = row.child(
                div()
                    .text_size(sp(12.0))
                    .text_color(theme.text_secondary)
                    .child(format!(
                        "{} {}",
                        tr!("boss.instruction_evidence"),
                        resolution.evidence
                    )),
            );
        }
        if live {
            let editable = pin.clone();
            let revision = pin.revision;
            let edit_label = if pin.state == InstructionPinState::Candidate {
                tr!("boss.instruction_review")
            } else {
                tr!("boss.instruction_edit")
            };
            let retire_label = if pin.state == InstructionPinState::Candidate {
                tr!("boss.instruction_dismiss")
            } else {
                tr!("boss.instruction_retire")
            };
            row = row.child(
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        boss_button(format!("instruction-edit-{id}"), edit_label.clone(), &theme)
                            .child(edit_label)
                            .on_activation(cx, move |this, window, cx| {
                                this.edit_instruction_pin(key, Some(editable.clone()), window, cx)
                            }),
                    )
                    .child(
                        boss_button(
                            format!("instruction-retire-{id}"),
                            retire_label.clone(),
                            &theme,
                        )
                        .child(retire_label)
                        .on_activation(cx, move |this, _, cx| {
                            this.boss_request(
                                key,
                                BossOperation::InstructionPin {
                                    action: Action::Retire {
                                        id,
                                        expected_revision: revision,
                                        evidence: "Explicitly retired or dismissed by the user"
                                            .into(),
                                        superseded_by: None,
                                    },
                                },
                                BossReply::Instructions,
                                cx,
                            );
                        }),
                    ),
            );
        }
        row.into_any_element()
    }

    fn render_instruction_pin_picker(&self, scope: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = &self.boss_ui.pin_editor else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        let (id, options, selected, default) = if scope {
            (
                "instruction-scope",
                editor.projects.clone(),
                editor.project_id,
                tr!("boss.instruction_all_projects"),
            )
        } else {
            (
                "instruction-trigger",
                editor.outcomes.clone(),
                editor
                    .retire_on
                    .map(|InstructionPinRetirement::OutcomeCompleted { outcome_id }| outcome_id),
                tr!("boss.instruction_manual_release"),
            )
        };
        let label = if scope {
            editor.project_label.clone()
        } else {
            editor.outcome_label.clone()
        }
        .unwrap_or_else(|| default.clone());
        let weak = cx.entity().downgrade();
        let trigger = MenuChip::new(id)
            .label(label)
            .outlined()
            .background(theme.raised);
        let items = move |_: &mut App| {
            std::iter::once((None, default.clone()))
                .chain(options.iter().map(|(id, label)| (Some(*id), label.clone())))
                .map(|(choice, label)| {
                    let weak = weak.clone();
                    MenuItem::new(label.clone(), move |_, cx| {
                        if let Some(entity) = weak.upgrade() {
                            entity.update(cx, |this, cx| {
                                if let Some(editor) = &mut this.boss_ui.pin_editor {
                                    if scope {
                                        editor.project_id = choice;
                                        editor.project_label = choice.map(|_| label.clone());
                                    } else {
                                        editor.retire_on = choice.map(|outcome_id| {
                                            InstructionPinRetirement::OutcomeCompleted {
                                                outcome_id,
                                            }
                                        });
                                        editor.outcome_label = choice.map(|_| label.clone());
                                        editor.automatic_condition = choice
                                            .map(|_| format!("Outcome '{label}' is completed"));
                                        if let Some(condition) = &editor.automatic_condition {
                                            editor.condition.update(cx, |input, cx| {
                                                input.set_content(condition.clone(), cx)
                                            });
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }
                    })
                    .selected(choice == selected)
                })
                .collect()
        };
        if scope {
            let handle = self.project_picker_handle(id, cx);
            project_dropdown_menu(
                trigger.selected(handle.is_open()),
                &handle,
                MenuAlign::BelowLeft,
                ProjectPickerVariant::ListFilter,
                items,
            )
        } else {
            let handle = self.menu_handle(id, cx);
            dropdown_menu(
                trigger.selected(handle.is_open()),
                id,
                &handle,
                MenuAlign::BelowLeft,
                items,
            )
        }
    }

    fn render_instruction_pin_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = &self.boss_ui.pin_editor else {
            return div().into_any_element();
        };
        let theme = Theme::current(cx);
        div()
            .id("instruction-editor")
            .flex_none()
            .max_h(px(420.0))
            .overflow_y_scroll()
            .px(px(20.0))
            .py(px(12.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .bg(theme.inset)
            .child(tr!("boss.instruction_text"))
            .child(boss_input(editor.instruction.clone(), &theme))
            .child(tr!("boss.instruction_scope"))
            .child(self.render_instruction_pin_picker(true, cx))
            .child(tr!("boss.instruction_auto_release"))
            .child(self.render_instruction_pin_picker(false, cx))
            .child(
                div()
                    .text_size(sp(11.0))
                    .text_color(theme.text_tertiary)
                    .child(tr!("boss.instruction_release_hint")),
            )
            .child(tr!("boss.instruction_condition"))
            .child(boss_input(editor.condition.clone(), &theme))
            .when_some(editor.source.clone(), |form, (quote, shortened)| {
                form.child(
                    div()
                        .text_size(sp(11.0))
                        .text_color(theme.text_tertiary)
                        .child(if shortened {
                            tr!("boss.instruction_source_excerpt")
                        } else {
                            tr!("boss.instruction_source")
                        }),
                )
                .child(
                    div()
                        .text_size(sp(12.0))
                        .text_color(theme.text_secondary)
                        .child(quote),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        boss_button("instruction-save", tr!("boss.instruction_save"), &theme)
                            .child(tr!("boss.instruction_save"))
                            .on_activation(cx, |this, _, cx| this.save_instruction_pin(cx)),
                    )
                    .child(
                        boss_button("instruction-cancel", tr!("boss.instruction_cancel"), &theme)
                            .child(tr!("boss.instruction_cancel"))
                            .on_activation(cx, |this, window, cx| {
                                this.boss_ui.pin_editor = None;
                                if let Some(focus) = &this.boss_ui.focus {
                                    window.focus(focus, cx);
                                }
                                cx.notify();
                            }),
                    ),
            )
            .into_any_element()
    }
}
