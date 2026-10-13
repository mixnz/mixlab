---
status: implemented
date: 2026-10-13
task: T207b
---

# `render.rs` split by command, its tests beside what they test (design)

`crates/mixengine-cli/src/render.rs` is 7,741 lines: 153 top-level items — 87 public functions,
5 public types, 61 private helpers — and 119 of the crate's 159 unit tests, in three test modules
(`tests`, `autostart_tests`, `grant_problems`) ordered by when each answer was written rather than
by which command prints it. T207a gave every command a `commands/<name>.rs`; this design gives each
one the `render/<name>.rs` beside it, and decides the three things the
[parent design](2026-10-13-files-that-fit-in-one-read-design.md) left open: where each function
goes, where the helpers several commands share go, and where each test goes.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207b. Everything the
parent design's D7 and D8 say about T207a holds here — a helper goes where its callers are, and a
commit only moves — and is not repeated beyond what applies.

## How the mapping was measured

Not by searching for names. On a scratch copy of `render.rs`, every top-level item was renamed with
a suffix (`site_list` → `site_list__zz`), and `cargo check -p mixengine-cli --all-targets
--message-format=json` was run. Every place that names an item then fails to resolve, and the
compiler's own span for each failure, mapped to the item enclosing it, is a call site. The same was
done a second time for the helpers inside the three test modules (fixtures such as `summary` or
`a_plan`), which says which test uses which fixture.

That gives 519 uses of the 153 items and 128 uses of the 20 test fixtures, measured on `master` at
`a5f1f92c`, including the uses that a name search gets wrong: `status`, `size`, `state`, `list`,
`names` and `table` are also local variables and field names all over the file. No item is
`#[cfg]`-gated on an operating system, so what compiles on Windows is the whole graph.

## D1 — `render/<name>.rs`, beside `commands/<name>.rs`

```
crates/mixengine-cli/src/
  render/
    mod.rs           the module docs (`//!`) and `mod` lines, nothing else
    table.rs         the listing every table is printed with, and the dash for a missing cell
    size.rs          a byte count at the scale a download is read in
    time.rs          how long ago, how long until, how long up
    catalogue.rs     what `mix runtime available` and `mix package available` share
    prerequisites.rs what an install lacks here — beside the crate-root prerequisites.rs
    upgrades.rs      what an update will do or did — beside the crate-root upgrades.rs
    <command>.rs     one per command that prints something: 22 of the 23, named as in commands/
```

Twenty-two, not twenty-three: `commands/logs.rs` prints the daemon's log lines as they come and
nothing in `render.rs` is its. `commands/status.rs`'s two answers, `status` and `storage`, both
land in `render/status.rs`.

`render.rs` becomes `render/mod.rs` in the last commit, when it holds nothing but its header and
its `mod` lines — `commands/mod.rs`'s shape. Until then it stays `render.rs`, the parent of the new
`render/` directory, which Rust allows; that keeps every commit before the last a plain shrink of a
baselined file (see D7).

## D2 — the mapping, item by item

The rule is the parent design's D7, applied to the measured graph: **one caller module, the item
lives there; several, and it is about one command's subject, it lives in that command's module and
the others import it; several, and it belongs to no command, it gets a module of its own under a
name that says what it does.** "Reached from" below names the `commands/` module, the crate-root
module (`crate::…`) or the other `render/` module that uses an item of that row.

| `render/` | Items (pub(crate) in **bold**) | Reached from |
| --- | --- | --- |
| `table.rs` | MISSING, table, table_of | render::catalogue, render::disk, render::extension, render::job, render::metrics, render::package, render::runtime, render::self_update, render::service, render::size |
| `size.rs` | size | render::disk, render::doctor, render::package, render::runtime, render::self_update, render::uninstall, render::upgrades |
| `time.rs` | ago, in_time, uptime, units | render::job, render::metrics, render::package, render::runtime, render::service, render::site, render::status |
| `catalogue.rs` | **Lines**, more, update_lines, MORE_HEADING, RUNS_HEADING, NEEDS_HEADING, needs, any_lacking, emulation_column, runs, **on_disk** | package, runtime, render::package, render::runtime |
| `prerequisites.rs` | **requirements**, **advisories** | blueprint, crate::prerequisites, render::upgrades |
| `upgrades.rs` | **upgrade_plan**, upgrade_item, upgrade_outcome | crate::upgrades |
| `service.rs` | **Walked**, **service_list**, yes_no, **service_autostart**, **save_resources**, **service_status**, **front_end**, **front_end_report**, **service_walk**, state, in_the_run_it_names, **service_found**, **service_adopted**, **service_creation**, **service_removal**, **service_idle**, probe, idle_line, **service_limits**, watchdog_line, limit_line, enforcement, counted, names | blueprint, extension, service, render::daemon, render::self_update |
| `elevation.rs` | **elevation_status**, waiting, **elevation_prompt**, grant, operations | blueprint, cert, elevation, render::job, render::status |
| `job.rs` | **job_list**, **job_status**, **job_succeeded** | blueprint, cert, crate::upgrades, disk, doctor, elevation, extension, job, package, runtime, service, uninstall |
| `runtime.rs` | **runtime_list**, **extension_list**, **extension_change**, RUNTIME_HEADINGS, LACKS_HEADING, lacks, **Devkit**, **lacks_after_install**, **runtime_catalogue**, **runtime_summary**, **runtime_resolved**, **runtime_removal** | runtime, render::job |
| `blueprint.rs` | **blueprint_captured**, **blueprint_imported**, **blueprint_list**, **blueprint_plan**, **blueprint_applied**, with_site, next_steps_said, **blueprint_had_a_failed_step**, disposition_word, step_said, action_said, site_kind_word | blueprint, render::project |
| `cert.rs` | **cert_issue**, **cert_status**, short, advice, **ca_status**, **ca_rotate**, **ca_uninstall**, browser, certificate, unusable | cert |
| `status.rs` | patterns, **status**, **status_json**, client, **storage** | status |
| `self_update.rs` | **update_status**, **update_handed_over**, **update_applied**, list, services | self_update |
| `daemon.rs` | **daemon_shutdown**, **credential_store_change** | daemon |
| `site.rs` | **site_failures**, **site_list**, owner_word, route_target_word, kind_word, https_word, **site_detail**, **site_removal**, **site_shared**, qr | domain, site |
| `package.rs` | **package_list**, **package_catalogue**, PACKAGE_HEADINGS, **package_removal**, **package_summary** | package |
| `home.rs` | **home_previous**, **home_restored** | home |
| `path.rs` | **Pathed**, **path_report** | path |
| `metrics.rs` | **metrics_frame**, **metrics_history**, percent, memory | metrics |
| `uninstall.rs` | **uninstall_removing**, **uninstall_still_removing**, **uninstall_report** | uninstall |
| `project.rs` | **project_list**, **project_detail**, **project_removal**, **project_export** | project |
| `doctor.rs` | **doctor**, **repair**, **bundle** | doctor |
| `disk.rs` | **disk_usage**, reclaim, **cleanup_report** | disk |
| `domain.rs` | **domain_status** | domain |
| `database.rs` | **database_created**, **database_credentials**, where_kept, **database_client**, **database_opened**, desktop_client | database |
| `extension.rs` | **extension_inspection**, **installed_extensions**, **extension_catalogue**, **extension_plan**, **extension_removal** | extension |
| `autostart.rs` | **Autostarted**, **autostart_report**, mechanism | autostart |

Where the rule left a choice, and what was chosen:

| Item | Reached from | Goes to | Why |
| --- | --- | --- | --- |
| `Walked`, `service_walk` | service, blueprint, extension, `render::daemon` | `service.rs` | a walk is starting and stopping services; the others ask for one |
| `names` | `render::service`, `render::self_update` | `service.rs` | a list of `ServiceId`s |
| `site_detail` | site, domain | `site.rs` | `mix domain status` prints a site |
| `elevation_prompt`, `grant`, `operations` | elevation, blueprint, cert, `render::job`, `render::status` | `elevation.rs` | all three are about the elevation queue |
| `job_status`, `job_succeeded` | twelve modules | `job.rs` | `mix job status` is the command whose answer it is; T207a's crate-root `jobs.rs` follows a job and prints nothing |
| `runtime_summary` | runtime, `render::job` | `runtime.rs` | a job's result can be a runtime |
| `next_steps_said` | `render::blueprint`, `render::project` | `blueprint.rs` | the steps a blueprint left; a project shows the ones it kept |
| `requirements`, `advisories` | blueprint, `crate::prerequisites`, `render::upgrades` | `prerequisites.rs` | what the crate-root `prerequisites.rs` asks about, printed |
| `upgrade_plan` and its two helpers | `crate::upgrades` | `upgrades.rs` | runtime's and package's alike, as in T207a |
| `Lines`, `on_disk`, the shared headings and cells | runtime, package | `catalogue.rs` | the release table both listings print; `RUNTIME_HEADINGS`, `LACKS_HEADING` and `lacks` are runtime's alone and go to `runtime.rs`, `PACKAGE_HEADINGS` to `package.rs` |
| `extension_list`, `extension_change` | runtime | `runtime.rs` | `mix runtime ext …` — a PHP extension, not an `extension` |
| `credential_store_change` | daemon | `daemon.rs` | `mix daemon credential-store` |
| `storage` | status | `status.rs` | `commands/status.rs` holds `mix storage` |
| **`uptime`** | `render::status` only | **`time.rs`** | the one exception to "one caller, it lives there": it is `units` under another name, and `units`' documentation says it is shared by `uptime` and `ago` so that the two round alike. Moving it to `status.rs` splits one rounding across two files and turns two plain doc links into cross-module ones |

The largest file this produces is `render/service.rs`, at about 1,200 lines with its tests — under
the limit, so nothing enters the baseline, but over the 1,000 that T207a's files all stayed under.
It is the service command's whole surface (list, status, walk, create, adopt, idle, limits); a
further split of `idle`/`limits` would be a design of its own and is not this task.

## D3 — what several commands share

Six modules belong to no command, and they hold only what the measured graph says is shared:

| Module | Items | Used by |
| --- | --- | --- |
| `table.rs` | `table`, `table_of`, `MISSING` | ten `render/` modules |
| `size.rs` | `size` | seven |
| `time.rs` | `ago`, `in_time`, `uptime`, `units` | seven |
| `catalogue.rs` | `Lines`, `more`, `update_lines`, `MORE_HEADING`, `RUNS_HEADING`, `NEEDS_HEADING`, `needs`, `any_lacking`, `emulation_column`, `runs`, `on_disk` | runtime, package |
| `prerequisites.rs` | `requirements`, `advisories` | blueprint, `crate::prerequisites`, `render::upgrades` |
| `upgrades.rs` | `upgrade_plan`, `upgrade_item`, `upgrade_outcome` | `crate::upgrades` |

Three small files rather than one: `size` and the time functions are each a different question
("will this fit", "how long"), the tables are a layout, and a module called `format` or `units`
that holds all three is `utils` by another name. `memory` (mebibytes with a decimal) stays in
`metrics.rs`, its only user; its doc's link to `size` becomes a path.

`render/mod.rs` itself holds no function. A helper in `mod.rs` would be reachable from everywhere
under a name that says nothing about it, which is the shape this split exists to undo.

## D4 — a test goes with the function it tests

**Every test moves into the `#[cfg(test)] mod tests` of the module that holds the function it is
written about**, in the same commit as that function. "The function it is written about" is the
one whose output it asserts on; an item that is only an argument or a value compared against
(`Lines`, `Walked`, `Devkit`, `MISSING`) does not decide it.

Measured: **no test asserts on two functions that land in different modules.** Three call two
functions, and each pair lands together — `a_failed_step_prints_its_exit_rather_than_a_skip`
(`blueprint_applied`, `blueprint_had_a_failed_step`), `a_database_in_a_file_store_says_so`
(`database_created`, `database_credentials`), `the_wait_for_a_removal_says_what_it_is_waiting_for`
(`uninstall_removing`, `uninstall_still_removing`). Three name a shared item besides the function
under test: `all_prints_every_release` and
`naming_a_line_prints_every_release_of_it_and_nothing_else` pass a `catalogue::Lines` to
`runtime_catalogue`, and `a_subject_with_no_cpu_figure_renders_a_dash_and_never_a_zero` compares
`metrics_frame`'s output with `table::MISSING`. They go with `runtime_catalogue` and
`metrics_frame`, and import the shared item.

| `render/` | Tests | Function under test → tests |
| --- | --- | --- |
| `table.rs` | 0 | — |
| `size.rs` | 0 | — |
| `time.rs` | 4 | `in_time`: `a_wait_is_rounded_the_way_an_age_is`, `a_deadline_already_gone_reads_as_a_wait`<br>`ago`: `an_age_is_rounded_the_way_an_uptime_is_and_never_reads_as_negative`<br>`uptime`: `uptime_stops_at_two_units_whichever_two_they_are` |
| `catalogue.rs` | 0 | — |
| `prerequisites.rs` | 2 | `requirements`: `what_is_lacking_is_said_with_what_can_be_done`<br>`advisories`: `a_warning_names_the_libraries_and_says_the_install_goes_on` |
| `upgrades.rs` | 3 | `upgrade_plan`: `a_plan_says_what_moves_and_what_becomes_of_the_old_version`, `a_plan_kept_for_a_reason_names_the_reason`, `a_finished_update_marks_each_line` |
| `service.rs` | 18 | `watchdog_line`: `a_watchdog_line_says_what_happens_at_the_end_of_it`<br>`service_creation`: `a_service_created_on_the_port_it_wanted_is_reported_without_a_story`, `a_service_moved_off_its_preferred_port_names_the_program_that_took_it`, `a_move_with_nothing_to_name_still_says_the_port_was_taken`<br>`service_list`: `a_failed_service_is_followed_by_why`, `the_listing_is_a_table_whose_columns_line_up_whatever_the_names_are`, `a_home_with_no_declarations_says_so_rather_than_printing_a_bare_heading`<br>`service_status`: `a_status_names_the_version_the_service_runs`, `a_service_that_was_never_created_is_told_apart_from_one_that_is_stopped`, `a_start_time_that_outlived_its_run_is_labelled_as_history_rather_than_as_the_present`, `a_row_naming_a_process_nothing_is_supervising_is_pointed_at_rather_than_smoothed_over`<br>`service_walk`: `a_walk_that_reached_everything_is_one_line`, `a_walks_failure_prints_the_runners_detail_first`, `a_walk_that_stopped_leads_with_the_service_to_fix_and_shows_what_it_took_down`, `a_refused_superuser_is_shown_with_the_repair_under_it`, `a_walk_nobody_waited_for_says_it_is_still_happening`<br>`service_idle`: `the_three_ways_of_never_idling_are_three_different_sentences`, `an_exemption_names_what_is_holding_the_service_open` |
| `elevation.rs` | 8 | `elevation_status`: `a_pending_list_says_what_each_operation_will_change`, `an_old_privileged_helper_is_reported_where_the_queue_is`, `a_current_privileged_helper_is_not_mentioned`, `a_machine_that_cannot_prompt_prints_what_to_do_instead`, `a_declined_grant_reads_as_a_choice_rather_than_a_failure`<br>`elevation_prompt`: `the_screen_before_a_prompt_is_the_list_and_what_the_prompt_will_be`<br>`grant`: `what_did_not_happen_is_printed_under_the_line`, `a_grant_that_did_everything_gains_no_lines` |
| `job.rs` | 0 | — |
| `runtime.rs` | 15 | `runtime_list`: `a_runtime_whose_folder_is_gone_reads_missing_and_names_the_way_back`<br>`runtime_catalogue`: `one_row_stands_for_each_line_with_a_count_of_the_rest`, `all_prints_every_release`, `naming_a_line_prints_every_release_of_it_and_nothing_else`, `an_update_is_named_above_the_table_with_the_command_that_applies_it`, `an_old_daemon_s_catalogue_is_printed_flat`, `a_kind_the_index_could_not_be_read_for_is_said_above_the_table`, `a_lacks_column_appears_only_when_a_row_lacks_something`, `a_needs_column_appears_only_when_a_row_lacks_something`, `a_catalogue_of_native_releases_has_no_column_about_it`, `a_daemon_that_reports_no_execution_brings_no_column_either`, `one_emulated_release_brings_the_column_and_the_sentence_that_explains_it`<br>`lacks_after_install`: `the_install_names_the_devkit_where_one_is_offered`, `the_install_says_nothing_when_the_devkit_is_installed`, `no_line_when_nothing_is_lacked` |
| `blueprint.rs` | 14 | `blueprint_applied`: `an_apply_with_steps_ends_with_what_to_run`, `an_apply_prints_every_step_and_spells_out_what_did_not_run`, `a_done_step_with_a_note_prints_it_under_the_step`<br>`next_steps_said`: `untrusted_steps_are_read_before_they_are_run`, `a_step_with_a_site_says_it`<br>`blueprint_applied + blueprint_had_a_failed_step`: `a_failed_step_prints_its_exit_rather_than_a_skip`<br>`blueprint_imported`: `an_import_says_whether_anything_vouched_for_it`, `an_import_says_which_kind_of_untrusted_it_is`<br>`blueprint_list`: `the_listing_says_which_kind_of_untrusted_each_one_is`, `a_home_with_no_blueprints_says_how_to_make_one`<br>`blueprint_plan`: `a_plan_prints_one_line_per_step_and_says_who_holds_a_taken_domain`, `a_plan_with_two_sites_names_each_one`, `a_plan_that_would_ask_for_a_password_says_so_once`, `enabling_an_extension_says_that_it_reaches_past_this_project` |
| `cert.rs` | 0 | — |
| `status.rs` | 9 | `status`: `status_names_the_credential_store_when_the_daemon_says`, `status_names_the_version_that_is_waiting`, `status_says_nothing_about_updates_when_there_is_nothing_to_say`, `the_human_rendering_leads_with_whether_it_is_up_and_which_home_it_is`, `a_daemon_from_another_build_is_explained_rather_than_left_to_be_noticed`, `a_daemon_that_reported_neither_names_nor_elevation_says_so_and_invents_nothing`, `the_status_line_names_the_mechanism_this_home_resolves_through`, `the_status_line_says_how_many_are_waiting_and_whether_the_daemon_is_elevated`<br>`status_json`: `the_json_is_the_daemons_answer_untouched_under_one_key` |
| `self_update.rs` | 7 | `update_status`: `a_pkg_copy_is_offered_the_installer_rather_than_refused`, `an_installed_version_says_to_finish`, `a_managed_install_prints_the_daemons_own_reason`, `an_offer_carries_everything_somebody_needs_to_answer_it`<br>`update_handed_over`: `a_handover_prints_the_package_and_the_command_every_time`, `a_handover_with_nothing_opened_prints_the_command_as_the_way`<br>`update_applied`: `an_applied_update_says_the_helper_was_kept_and_why` |
| `daemon.rs` | 4 | `daemon_shutdown`: `a_shutdown_says_what_happened_to_the_daemon_and_puts_the_services_under_it`, `a_shutdown_with_nothing_to_stop_is_one_line_about_the_daemon`, `a_shutdown_that_could_not_be_ordered_is_told_apart_from_one_with_nothing_to_stop`, `a_service_that_would_not_stop_is_named_although_the_daemon_stopped_anyway` |
| `site.rs` | 7 | `site_list`: `a_site_listing_names_the_owner`<br>`site_shared`: `a_shared_site_prints_its_name_and_where_to_get_the_authority`, `a_share_with_a_deadline_says_when_it_ends`, `a_share_with_no_deadline_says_nothing_about_one`, `a_name_that_is_not_advertised_says_so`, `a_shared_site_prints_its_url_the_interface_and_a_code`<br>`site_failures`: `a_site_whose_pool_failed_says_which_and_why` |
| `package.rs` | 1 | `package_list`: `a_package_whose_folder_is_gone_reads_missing_and_names_the_way_back` |
| `home.rs` | 0 | — |
| `path.rs` | 0 | — |
| `metrics.rs` | 5 | `metrics_frame`: `a_subject_with_no_cpu_figure_renders_a_dash_and_never_a_zero`, `cpu_is_a_share_of_the_machine`, `a_frame_that_measured_nothing_says_so_rather_than_printing_an_empty_table`<br>`metrics_history`: `a_history_row_shows_how_many_readings_it_is_made_of`, `an_empty_history_says_how_long_this_home_keeps_one` |
| `uninstall.rs` | 1 | `uninstall_removing + uninstall_still_removing`: `the_wait_for_a_removal_says_what_it_is_waiting_for` |
| `project.rs` | 2 | `project_export`: `an_export_names_the_site_entries_it_kept`<br>`project_detail`: `a_project_lists_the_sites_its_manifest_declares` |
| `doctor.rs` | 0 | — |
| `disk.rs` | 4 | `disk_usage`: `the_disk_table_says_what_would_reclaim_each_row`, `an_unreadable_category_says_so_in_the_table`<br>`cleanup_report`: `a_cleanup_prints_every_row_and_ends_on_the_total`, `a_partial_cleanup_says_what_would_not_go` |
| `domain.rs` | 0 | — |
| `database.rs` | 3 | `database_created`: `a_created_database_says_where_the_password_lives`<br>`database_credentials`: `the_last_line_of_the_credentials_is_the_password_alone`<br>`database_created + database_credentials`: `a_database_in_a_file_store_says_so` |
| `extension.rs` | 7 | `installed_extensions`: `an_installed_extension_is_listed_with_what_it_is_for`<br>`extension_plan`: `an_extension_plan_names_the_site_it_would_take`<br>`extension_inspection`: `an_inspection_says_what_would_run_and_what_was_only_asked_for`, `the_permission_lines_do_not_read_as_grants`, `every_interface_is_explained`<br>`extension_catalogue`: `an_empty_registry_does_not_read_as_a_build_too_old`, `a_listing_this_build_cannot_read_still_says_to_update` |
| `autostart.rs` | 5 | `autostart_report`: `an_entry_for_another_home_is_not_reported_as_set_up`, `an_entry_for_this_home_reads_as_set_up_and_names_what_it_starts`, `an_enable_that_wrote_nothing_does_not_claim_to_have_written`, `an_enable_that_wrote_says_when_it_takes_effect`, `a_machine_with_no_mechanism_says_there_is_nothing_to_register` |

119 tests before, 119 after; the other 40 of the crate's 159 are in files this task does not touch.

### Fixtures

Seventeen of the twenty test helpers serve tests that all land in one module, and move with them.
Three are shared across modules:

| Fixture | Used by tests in | Goes to |
| --- | --- | --- |
| `id` | `service.rs`, `daemon.rs` (and by `summary`) | `render/service.rs` |
| `summary` | `service.rs`, `site.rs` (and by `failed_with`) | `render/service.rs` |
| `failed_with` | `service.rs`, `site.rs` | `render/service.rs` |

The same rule as for code: they build a `ServiceSummary`, so they live with the service tests, as
`pub(in crate::render)` inside a `pub(super) mod tests`, and the `site` and `daemon` tests import
them. The two fixtures called `a_plan` are different functions — a `BlueprintPlan` in `tests`, an
`UpgradePlan` in `grant_problems` — and land in `blueprint.rs` and `upgrades.rs`, so the clash
disappears rather than needing a rename.

### The test modules' names

Each new file has one `#[cfg(test)] mod tests`. `autostart_tests` and `grant_problems` do not
survive as names: the first becomes `render::autostart::tests`, and the second is split —
its two `grant` tests to `render::elevation::tests`, its three `upgrade_plan` tests to
`render::upgrades::tests` (which shows the name was already wrong for three of its five tests).
The test functions keep their names; their paths in `--list` change, their count does not.

## D5 — visibility, the narrowest that compiles

- A function a `commands/` module calls stays `pub(crate)`: 86 of the 87 public functions, and
  the five public types.
- An item only other `render/` modules reach becomes `pub(super)` — visible in `render` and below,
  nowhere else. Twenty items change; nineteen were private:
  `MISSING`, `table`, `size`, `ago`, `in_time`, `uptime` (shared modules — `units` is called only
  from inside `time.rs` and stays private);
  `more`, `update_lines`, `MORE_HEADING`, `RUNS_HEADING`, `NEEDS_HEADING`, `needs`, `any_lacking`,
  `emulation_column`, `runs` (catalogue, for runtime and package); `grant` (for `job_status`),
  `operations` (for `status`), `names` (for `self_update`), `next_steps_said` (for
  `project_detail`).
- `table_of`, the 87th, is the twentieth change: `pub(crate)` today with no caller outside
  `render.rs`, so `pub(super)` is narrower and still compiles.
- Everything else stays private to its new file.
- In `render/mod.rs`, a module with a `pub(crate)` item is `pub(crate) mod`; `table`, `size` and
  `time` have none and are plain `mod`.
- The three shared fixtures are `pub(in crate::render)`, the one place this crate will use that
  form: `pub(super)` inside `render::service::tests` would reach only `render::service`.

No `pub(crate) use` re-exports from `render/mod.rs` — see D6.

## D6 — callers name the module

A command reaches its rendering by path: `render::site_list(…)` becomes
`render::site::site_list(…)`, and `use crate::render;` stays as it is. About 160 call sites in
`commands/` and the crate root change that way, by a path segment and nothing else.

A glob re-export in `render/mod.rs` would leave every caller untouched, and it is what this design
rejects: the point of the split is that `render::site::site_list` says which file to open, and a
re-export puts the lookup this task removes back behind one more hop. It is also T207a's choice —
`crate::Choice` became `crate::commands::self_update::Choice`.

## D7 — a move, and only a move

What a commit may change is the parent design's D8 list, and here, in full:

1. Where an item is, with every line above it that belongs to it — its `///` lines, a plain `//`
   comment directly above it (`limit_line` has one, D8), a `#[must_use]` or `#[derive]`, and an
   `impl` block (`Lines`, `Walked`). There are five top-level
   `#[must_use]`s and no multi-line top-level attribute; the one `#[expect(` that spans lines sits
   inside `percent`'s body and moves with it.
2. Its visibility, per D5.
3. `use` lines, and a path that names a moved item: `render::foo` → `render::<module>::foo` in
   callers (D6), and in `render/` the `use super::table::table;` that replaces an unqualified call
   reaching across files — the body of the call does not change. That includes the `use` lines
   `render.rs` itself gains while it still holds code calling an item already moved, which go again
   when that code moves; and where a path segment makes a call longer than rustfmt's width,
   rustfmt's re-wrap of that one expression is part of the same change.
4. Links that now cross a file:
   - six intra-doc links, written as `crate::render::<module>::<item>` paths in the commit that
     moves the target: `daemon_shutdown` → `service_walk`, `package_summary` → `runtime_summary`,
     `metrics_history` → `ago`, `memory` → `size`, `cleanup_report` → `uninstall_report`,
     `Autostarted` → `Pathed`. The full path resolves the same from `render.rs` and from the
     linking item's later home, so the link is edited once; every target is visible from where it
     is linked (D5);
   - a doc link to a `mixengine_proto` type whose only mention in a new file is that link, which
     would leave its `use` unused, written as a path (`mixengine_proto::ServiceWalk`) instead;
   - one link into `render` from outside: `commands/cert.rs`'s ``[`render::job_status`]``;
   - two relative links to ADR 0019, in `runs`' doc and in a runtime test's comment, which gain one
     `../` because their file is one directory deeper — `check-docs.mjs` would not notice them
     stop resolving, since a link that leaves the repository's `docs/` is not checked;
   - one link into the code from an implemented spec,
     [the T77 design](2026-09-01-t77-blueprint-capture-design.md)'s
     `[render.rs](../../crates/mixengine-cli/src/render.rs)`, which `check-docs.mjs` *does* check
     and which goes dead when `render.rs` becomes `render/mod.rs`. It is pointed at the
     `render/` directory, which is what its sentence ("does not contain a single ✓") is about. It
     is the one edit to an implemented spec in this task, and only to keep a link alive.
5. The test module wrappers: `#[cfg(test)] mod tests {`, its `use super::*;` and the
   `mixengine_proto` names its tests use — the same kind of line as a `use`.
6. `render.rs`'s entry in `scripts/file-size-baseline.json`, lowered by `--shrink` in every commit,
   and removed by it once the file is under 1,500.

Nothing else: no rename, no signature, no body, no reordering inside a file — items keep the order
they have in `render.rs`, so `--color-moved` shows each one as a block. ADRs 0019 and 0021 name
`render.rs` and `render::status` in their prose, and four implemented specs (T49a, T54, T88c, T90)
name `render.rs` in tables or sentences without linking it; they are records and stay as written.

**One commit per `render/` module**, each green on its own and each running
`node scripts/check-file-sizes.mjs --shrink` after every other check has passed. Shared modules
first, so that a later move imports from where an item will stay rather than from `render.rs` and
then again from its home: `table`, `size`, `time`, `catalogue`, `prerequisites`, `upgrades`; then
the subjects others import from: `service`, `elevation`, `runtime`, `job`, `blueprint` — `runtime`
before `job`, because `job_status` calls `runtime_summary`; then the rest in the order they appear
in `render.rs`: `cert`, `status`, `self_update`, `daemon`, `site`,
`package`, `home`, `path`, `metrics`, `uninstall`, `project`, `doctor`, `disk`, `domain`,
`database`, `extension`, `autostart`. The last commit renames what is left of `render.rs` to
`render/mod.rs` and repoints the T77 link. Measured by a dry run of every move on a scratch copy,
`render.rs` drops under 1,500 with the `project` commit, the 22nd of 28 moves (1,461 lines), and
`--shrink` takes it out of the baseline there.

With this order, no new file ever imports from `render.rs`: everything a moved item calls in
another module has already moved. The temporary imports are all in `render.rs` itself — a `use`
line per module whose items the code still there calls, shrinking as that code leaves — and in its
test module, which from the `service` commit until the `daemon` and `site` commits imports `id`,
`summary` and `failed_with` from `render::service::tests`.

The pull request is squash-merged, as T207a's was.

## D8 — five doc comments that are wrong where they are

Found while measuring, and **not** fixed in a move — each is moved as the compiler sees it,
attached to the item below it, and fixed in one commit of its own after the last move:

| Where | What is wrong |
| --- | --- |
| above `cert_issue` | `ca_status`'s doc ("`mix cert ca-status`, for a person", two paragraphs) is stacked on top of `cert_issue`'s own; `ca_status` has none |
| above `service_found` | `service_creation`'s doc ("`mix service create`", the T34c paragraph) and a `#[must_use]` sit above `service_found`'s own doc; `service_creation` has neither |
| above `runtime_resolved` | `runtime_removal`'s doc ("`mix runtime uninstall`", the no-default paragraph) is stacked on `runtime_resolved`'s; `runtime_removal` has none |
| above `limit_line` | its one line of description is a plain `//` comment, so rustdoc shows `limit_line` undocumented |
| on `services` | it reads "1 operation is / 3 operations are", which is `operations`' wording; `services` returns "1 service" / "N services" |

Each pair lands in the same new file (`cert.rs`, `service.rs`, `runtime.rs`; `limit_line` in
`service.rs`; `services` in `self_update.rs`, its wording borrowed from `operations` in
`elevation.rs`), so moving them as they are loses nothing and the fix is a plain edit afterwards. The `#[must_use]` moving to `service_creation` is a change of behaviour in the narrowest
sense — clippy could then flag a discarded result — and is why the fix is a commit of its own.

## MixLab

No part in it. This changes how `mix`'s source is laid out and nothing it prints: no daemon method,
no type in `mixengine-proto` or `bindings/`, no flag, no output. MixLab draws from the API and
`bindings/`, never from `mix`, and `scripts/check-client-surface.mjs` gives the same answer before
and after.

## Testing

What proves nothing changed, before the first commit and after each:

- **The unit-test count**: `cargo test -p mixengine-cli --bins -- --list | grep -c ': test$'` is
  **159** before and after; of them, `grep -c '^render::'` is 119 before and after.
- **`bash packaging/docs.sh --check`** shows no diff. `render` holds no clap definition, so a diff
  here would mean something other than a move happened.
- **`git diff --color-moved=dimmed-zebra`** over each commit leaves undimmed only the lines D7
  lists: `use` lines, visibility, paths and links, test module wrappers, the baseline entry.
- **`bash scripts/gate.sh`**: clippy with `-D warnings` catches an import left unused and an item
  that became dead; rustdoc with `-D warnings` catches a doc link that no longer resolves; the
  ratchet catches a baseline entry left behind.
- **`private_interfaces`**: of the five public types, `Walked`, `Pathed` and `Autostarted` are
  fieldless enums, `Devkit` holds a `&str` and `Lines` a `bool` and an `Option<String>`; none
  carries a private type, so none trips the lint T207a met. A move that did would surface in
  clippy, and the answer is to move the field's type with it, not to widen it.
- **The integration suites** under `crates/mixengine-cli/tests/` drive the built `mix` and do not
  read its source; they run unchanged in CI's `test` and `system` legs.

No new tests: there is no new behaviour, and a test of the file layout would test nothing a user
sees.

## Not in scope

- Splitting `render/service.rs` further (D2).
- Fixing the five doc comments in a move (D8 — they get their own commit).
- Rewording any rendering, merging look-alike helpers (`list` and `names` differ only in their
  element type), or turning `table`'s const generic into something else.
- The daemon's `api/rpc.rs` and `services/mod.rs`, which the parent design already leaves out.

## Is there an ADR here? No

The rule this applies is the parent design's D7, and the standard behind it is
[docs/standards/rust.md](../standards/rust.md)'s *modules by capability*. Nothing cross-cutting is
decided.

## Documentation, when it lands

- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207b ticked, and its line pointing at this
  design; the phase's goal line and **M45** gain `render.rs` beside `main.rs`.
- This spec flipped to `implemented` in the commit that ticks T207b, and `docs/specs/README.md`
  regenerated.
- Nothing in `docs/standards/rust.md` or `CLAUDE.md`: neither names `render.rs`.
