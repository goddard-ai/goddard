CREATE TABLE `stable_runtime_stops` (
	`command_id` text PRIMARY KEY NOT NULL,
	`session_id` text NOT NULL,
	`namespace` text NOT NULL,
	`root` text NOT NULL,
	`runtime_instance` text NOT NULL,
	`runtime_session_id` text NOT NULL,
	`generation` integer NOT NULL,
	`requested_at` integer NOT NULL,
	`confirmed_at` integer
);
--> statement-breakpoint
CREATE UNIQUE INDEX `stable_runtime_stop_owner` ON `stable_runtime_stops` (`runtime_instance`,`runtime_session_id`,`generation`);