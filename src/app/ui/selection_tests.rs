use super::*;
use crate::app::{
    filter::{Filter, FilterCriterion},
    sorter::{SortCriteria, SortOrder},
    test::create_default_app,
};
use crossterm::event::{KeyCode, KeyModifiers};

fn assert_selection_matches_current<D: DataProvider>(ui: &UIComponents<'_>, app: &App<D>) {
    assert_eq!(
        ui.entries_list
            .state
            .selected()
            .and_then(|index| app.get_active_entries().nth(index))
            .map(|entry| entry.id),
        app.current_entry_id
    );
}

#[tokio::test]
async fn resetting_filter_keeps_highlight_and_delete_target_on_the_same_entry() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    app.apply_filter(Some(Filter {
        criteria: vec![FilterCriterion::Title("Title 1".into())],
        ..Default::default()
    }));
    ui.update_current_entry(&mut app);
    assert_eq!(ui.entries_list.state.selected(), Some(0));

    ui.handle_input(
        &Input::new(KeyCode::Char('x'), KeyModifiers::NONE),
        &mut app,
    )
    .await
    .unwrap();
    ui.update_current_entry(&mut app);
    assert_eq!(ui.entries_list.state.selected(), Some(1));
    assert_selection_matches_current(&ui, &app);

    ui.handle_input(
        &Input::new(KeyCode::Char('d'), KeyModifiers::NONE),
        &mut app,
    )
    .await
    .unwrap();
    ui.handle_input(
        &Input::new(KeyCode::Char('y'), KeyModifiers::NONE),
        &mut app,
    )
    .await
    .unwrap();
    ui.update_current_entry(&mut app);
    assert_eq!(
        app.get_active_entries()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        [1]
    );
    assert_selection_matches_current(&ui, &app);
}

#[tokio::test]
async fn selection_reconciliation_preserves_unsaved_text_across_filter_and_sort_changes() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    ui.editor.set_entry_content("Unsaved draft", &app);
    assert!(ui.has_unsaved());

    app.apply_sort(vec![SortCriteria::Title], SortOrder::Ascending);
    ui.update_current_entry(&mut app);
    assert_eq!(ui.entries_list.state.selected(), Some(0));
    assert_selection_matches_current(&ui, &app);
    assert_eq!(ui.editor.get_content(), "Unsaved draft");
    assert!(ui.has_unsaved());

    app.cycle_tags_in_filter();
    ui.update_current_entry(&mut app);
    assert_selection_matches_current(&ui, &app);
    app.apply_filter(None);
    ui.update_current_entry(&mut app);
    assert_selection_matches_current(&ui, &app);
    assert_eq!(ui.editor.get_content(), "Unsaved draft");
    assert!(ui.has_unsaved());
}

#[tokio::test]
async fn filtering_out_every_entry_clears_the_highlight() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    app.apply_filter(Some(Filter {
        criteria: vec![FilterCriterion::Title("No matching title".into())],
        ..Default::default()
    }));
    ui.update_current_entry(&mut app);
    assert_selection_matches_current(&ui, &app);
    assert_eq!(ui.entries_list.state.selected(), None);
}

#[tokio::test]
async fn applying_a_filter_with_multiple_matches_reconciles_the_existing_selection() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.add_entry("Another entry".into(), chrono::Utc::now(), Vec::new(), None)
        .await
        .unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    assert_eq!(ui.entries_list.state.selected(), Some(2));
    app.apply_filter(Some(Filter {
        criteria: vec![FilterCriterion::Title("Title".into())],
        ..Default::default()
    }));
    ui.update_current_entry(&mut app);
    assert_eq!(app.get_active_entries().count(), 2);
    assert_eq!(ui.entries_list.state.selected(), Some(1));
    assert_selection_matches_current(&ui, &app);
}
