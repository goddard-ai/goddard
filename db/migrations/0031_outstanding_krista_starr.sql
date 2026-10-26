ALTER TABLE `sessions` ADD `metadata_revision` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE `sessions` ADD `metadata_mutation_id` text;