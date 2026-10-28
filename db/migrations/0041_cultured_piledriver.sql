CREATE TABLE `workspace_cleanup_runs` (
	`session_id` text NOT NULL,
	`generation` text NOT NULL,
	`phase` text NOT NULL,
	`payload` text NOT NULL,
	PRIMARY KEY(`session_id`, `generation`)
);
