ALTER TABLE `sessions` ADD `models` text;--> statement-breakpoint
DROP INDEX IF EXISTS `sessions_repository_idx`;--> statement-breakpoint
DROP INDEX IF EXISTS `sessions_repository_pr_number_idx`;--> statement-breakpoint
CREATE INDEX `idx_sessions_repository_pr_number` ON `sessions` (`repository`,`prNumber`);