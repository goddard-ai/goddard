CREATE TABLE `workspace_cleanup_intents` (
	`session_id` text PRIMARY KEY NOT NULL,
	`generation` text NOT NULL,
	`project_id` text NOT NULL,
	`project_path` text,
	`workspace` text,
	`archived_at` integer NOT NULL,
	`phase` text DEFAULT 'pending' NOT NULL,
	`retry_at` integer DEFAULT 0 NOT NULL,
	`failures` integer DEFAULT 0 NOT NULL,
	`last_error` text
);
--> statement-breakpoint
CREATE INDEX `workspace_cleanup_ready` ON `workspace_cleanup_intents` (`phase`,`retry_at`);