CREATE TABLE `transcript_entries` (
	`session_id` text NOT NULL,
	`seq` integer NOT NULL,
	`entry_id` text NOT NULL,
	`kind` text NOT NULL,
	`turn_id` text,
	`after_message` integer NOT NULL,
	`block_index` integer,
	`activity_index` integer,
	`revision` integer NOT NULL,
	`fingerprint` integer NOT NULL,
	`compact` text,
	`compact_bytes` integer DEFAULT 0 NOT NULL,
	PRIMARY KEY(`session_id`, `seq`)
);
--> statement-breakpoint
CREATE INDEX `transcript_entries_by_id` ON `transcript_entries` (`session_id`,`entry_id`);--> statement-breakpoint
CREATE INDEX `transcript_entries_by_turn` ON `transcript_entries` (`session_id`,`turn_id`,`seq`);--> statement-breakpoint
CREATE TABLE `transcript_heads` (
	`session_id` text PRIMARY KEY NOT NULL,
	`revision` integer NOT NULL,
	`entry_count` integer NOT NULL,
	`message_count` integer NOT NULL,
	`queued_messages` text DEFAULT '[]' NOT NULL,
	`details_pruned` integer DEFAULT false NOT NULL
);
--> statement-breakpoint
CREATE TABLE `transcript_payloads` (
	`session_id` text NOT NULL,
	`entry_id` text NOT NULL,
	`field` text NOT NULL,
	`revision` integer NOT NULL,
	`fingerprint` integer NOT NULL,
	`bytes` integer NOT NULL,
	`truncated` integer DEFAULT false NOT NULL,
	`content` blob NOT NULL,
	PRIMARY KEY(`session_id`, `entry_id`, `field`)
);
--> statement-breakpoint
CREATE INDEX `transcript_payloads_by_session` ON `transcript_payloads` (`session_id`);--> statement-breakpoint
CREATE TABLE `transcript_turns` (
	`session_id` text NOT NULL,
	`turn_id` text NOT NULL,
	`turn_count` integer NOT NULL,
	`position` integer NOT NULL,
	`data` text NOT NULL,
	PRIMARY KEY(`session_id`, `turn_id`)
);
--> statement-breakpoint
CREATE INDEX `transcript_turns_by_count` ON `transcript_turns` (`session_id`,`turn_count`);