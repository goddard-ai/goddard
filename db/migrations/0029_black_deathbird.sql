CREATE TABLE `stable_runtime_links` (
	`session_id` text PRIMARY KEY NOT NULL,
	`namespace` text NOT NULL,
	`runtime_session_id` text NOT NULL,
	`runtime_instance` text NOT NULL,
	`generation` integer NOT NULL,
	`applied_seq` integer DEFAULT 0 NOT NULL,
	`status` text NOT NULL,
	`updated_at` integer NOT NULL
);
--> statement-breakpoint
CREATE TABLE `stable_runtime_receipts` (
	`request_id` text PRIMARY KEY NOT NULL,
	`session_id` text NOT NULL,
	`generation` integer NOT NULL,
	`class` text NOT NULL,
	`outcome` text NOT NULL,
	`recorded_at` integer NOT NULL
);
