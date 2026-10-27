CREATE TABLE `checkpoint_workspace_writers` (
	`origin_id` text PRIMARY KEY NOT NULL,
	`session_id` text NOT NULL,
	`turn_count` integer NOT NULL,
	`workspace` text NOT NULL,
	`identity` text,
	`confirmed` integer DEFAULT 0 NOT NULL,
	`created_at` integer NOT NULL
);
--> statement-breakpoint
CREATE INDEX `checkpoint_unconfirmed_writers` ON `checkpoint_workspace_writers` (`origin_id`) WHERE "checkpoint_workspace_writers"."confirmed" is not 1;