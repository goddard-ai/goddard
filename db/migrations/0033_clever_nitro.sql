CREATE TABLE IF NOT EXISTS `boss_coordinator` (
	`id` integer PRIMARY KEY NOT NULL,
	`schema_version` integer NOT NULL,
	`revision` integer NOT NULL,
	`data` text NOT NULL
);
--> statement-breakpoint
CREATE TABLE IF NOT EXISTS `boss_payloads` (
	`digest` text PRIMARY KEY NOT NULL,
	`data` text NOT NULL
);
--> statement-breakpoint
CREATE TABLE IF NOT EXISTS `boss_records` (
	`collection` text NOT NULL,
	`parent_id` text NOT NULL,
	`id` text NOT NULL,
	`position` integer NOT NULL,
	`lifecycle` text,
	`data` text NOT NULL,
	PRIMARY KEY(`collection`, `parent_id`, `id`)
);
--> statement-breakpoint
CREATE INDEX IF NOT EXISTS `boss_records_lifecycle` ON `boss_records` (`collection`,`lifecycle`);