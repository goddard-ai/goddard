CREATE TABLE `repo_events` (
	`id` integer PRIMARY KEY AUTOINCREMENT,
	`github_username` text NOT NULL,
	`event_json` text NOT NULL,
	`created_at` text NOT NULL
);
