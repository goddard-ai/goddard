ALTER TABLE `stable_runtime_stops` ADD `retired_at` integer;--> statement-breakpoint
ALTER TABLE `stable_runtime_stops` ADD `retry_at` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE `stable_runtime_stops` ADD `failures` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE `stable_runtime_stops` ADD `last_error` text;--> statement-breakpoint
CREATE INDEX `stable_runtime_stop_ready` ON `stable_runtime_stops` (`retired_at`,`retry_at`,`command_id`);