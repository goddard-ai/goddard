CREATE TABLE `native_workspace_writers` (
	`origin_id` text PRIMARY KEY NOT NULL,
	`session_id` text NOT NULL,
	`runtime_id` text NOT NULL,
	`workspace` text NOT NULL,
	`identity` text,
	`confirmed` integer DEFAULT 0 NOT NULL,
	`created_at` integer NOT NULL
);
--> statement-breakpoint
CREATE INDEX `native_unconfirmed_writers` ON `native_workspace_writers` (`origin_id`) WHERE "native_workspace_writers"."confirmed" is not 1;