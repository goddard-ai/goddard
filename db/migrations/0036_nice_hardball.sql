CREATE TABLE `stable_runtime_writers` (
	`writer_key` text PRIMARY KEY NOT NULL,
	`session_id` text NOT NULL,
	`root` text NOT NULL,
	`runtime_instance` text NOT NULL,
	`runtime_session_id` text NOT NULL,
	`generation` integer NOT NULL,
	`workspace` text NOT NULL,
	`created_at` integer NOT NULL
);
