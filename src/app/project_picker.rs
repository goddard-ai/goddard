//! App-specific project choices and selection effects for the shared picker.
use super::*;
use crate::ui::project_picker::{ProjectPickerContent, ProjectPickerOption, project_popover};

impl Waku {
    pub(super) fn composer_project_label(&self, project: &Project) -> String {
        let name = if project.is_projectless() {
            tr!("project.choose_project")
        } else {
            project.display_name()
        };
        match self.daemons.project_owner(project.id) {
            waku_client::DaemonKey::Remote(host) => self
                .remote_host_name(host)
                .map(|host| format!("{name} · {host}"))
                .unwrap_or(name),
            waku_client::DaemonKey::Local => name,
        }
    }

    pub(super) fn project_picker_handle(
        &self,
        id: impl Into<SharedString>,
        cx: &mut App,
    ) -> ProjectPickerHandle {
        let id = id.into();
        if let Some(handle) = self.project_pickers.borrow().get(&id) {
            return handle.clone();
        }
        let handle = ProjectPickerHandle::new(self.menu_handle(id.clone(), cx));
        // Keyboard shortcuts find the same handle, including its panel lifecycle.
        self.menus
            .borrow_mut()
            .insert(id.clone(), handle.menu().clone());
        self.project_pickers.borrow_mut().insert(id, handle.clone());
        handle
    }

    #[track_caller]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_task_project_picker<E>(
        &self,
        trigger: E,
        handle: &ProjectPickerHandle,
        align: MenuAlign,
        variant: ProjectPickerVariant,
        mut projects: Vec<Project>,
        selected: Option<Uuid>,
        show_no_project: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement
    where
        E: ParentElement + Styled + InteractiveElement + IntoElement + 'static,
    {
        debug_assert!(matches!(
            variant,
            ProjectPickerVariant::NewTask | ProjectPickerVariant::TaskProject
        ));
        let new_task = matches!(variant, ProjectPickerVariant::NewTask);
        if !projects.is_empty() {
            // Decay once per project, rather than in the sort comparator. All
            // scores decay at the same rate, so repainting cannot change order.
            let usage_scores = new_task.then(|| {
                let now = waku_protocol::model::unix_time();
                projects
                    .iter()
                    .map(|project| {
                        (
                            project.id,
                            self.state.new_task_project_score(project.id, now),
                        )
                    })
                    .collect::<HashMap<_, _>>()
            });
            super::composer::project_picker_order(
                &mut projects,
                &self.state.sessions,
                selected,
                usage_scores.as_ref(),
            );
        }
        let projects = projects
            .into_iter()
            .map(|project| {
                let id = project.id;
                let weak = cx.entity().downgrade();
                let star_weak = weak.clone();
                let remote = self.is_remote_project(id);
                let detail = match self.daemons.project_owner(id) {
                    waku_client::DaemonKey::Remote(host) => {
                        self.remote_host_name(host).map(SharedString::from)
                    }
                    waku_client::DaemonKey::Local => None,
                };
                ProjectPickerOption {
                    key: id.to_string().into(),
                    search_text: format!(
                        "{} {}",
                        project.display_name(),
                        settings::abbreviate_home_path(
                            &project.path,
                            self.home_directory.as_deref()
                        )
                    )
                    .to_lowercase(),
                    detail,
                    item: MenuItem::new(project.display_name(), move |window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if new_task {
                                this.state.record_new_task_project_pick(
                                    id,
                                    waku_protocol::model::unix_time(),
                                );
                                this.save();
                            }
                            if Some(id) != selected {
                                this.select_project_from_composer(id, window, cx);
                            }
                        });
                    })
                    .icon(if remote {
                        "icons/server.svg"
                    } else if project.is_friends() || project.friend_peer_id.is_some() {
                        "icons/friends.svg"
                    } else {
                        "icons/folder.svg"
                    })
                    .selected(Some(id) == selected)
                    .star(project.starred, move |_, cx| {
                        let _ =
                            star_weak.update(cx, |this, cx| this.toggle_project_starred(id, cx));
                    }),
                }
            })
            .collect::<Vec<_>>();
        let weak = cx.entity().downgrade();
        let mut footer = vec![ProjectPickerOption::from(
            MenuItem::new(tr!("project.new_project"), move |_, cx| {
                let _ = weak.update(cx, |this, cx| this.add_project(cx));
            })
            .icon("icons/folder-new.svg")
            .shortcut_action(&NewProject),
        )];
        if show_no_project {
            let projectless_selected = selected.is_some_and(|id| {
                self.state
                    .projects
                    .iter()
                    .any(|project| project.id == id && project.is_projectless())
            });
            let weak = cx.entity().downgrade();
            footer.push(ProjectPickerOption::from(
                MenuItem::new(tr!("project.no_project"), move |_, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if projectless_selected {
                            this.disable_composer_incognito(cx);
                        } else {
                            this.create_projectless_session_from_composer(cx);
                        }
                    });
                })
                .icon("icons/x.svg")
                .selected(projectless_selected),
            ));
        }
        let projects = Rc::new(projects);
        project_popover(trigger, handle, align, move |_| ProjectPickerContent {
            projects: projects.clone(),
            leading: Vec::new(),
            footer: footer.clone(),
        })
    }
}
