CREATE TABLE `terminal_workspace_writers` (
	`workspace` text NOT NULL,
	`identity` text NOT NULL,
	`created_at` integer NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `terminal_workspace_writer_origin` ON `terminal_workspace_writers` (`workspace`,`identity`);