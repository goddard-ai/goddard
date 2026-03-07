CREATE TABLE `auth_sessions` (
	`token` text PRIMARY KEY,
	`github_user_id` integer NOT NULL,
	`github_username` text NOT NULL,
	`expires_at` integer NOT NULL,
	`created_at` text NOT NULL,
	CONSTRAINT `fk_auth_sessions_github_user_id_users_github_user_id_fk` FOREIGN KEY (`github_user_id`) REFERENCES `users`(`github_user_id`)
);
--> statement-breakpoint
CREATE TABLE `pull_requests` (
	`id` integer PRIMARY KEY AUTOINCREMENT,
	`number` integer NOT NULL,
	`owner` text NOT NULL,
	`repo` text NOT NULL,
	`title` text NOT NULL,
	`body` text,
	`head` text NOT NULL,
	`base` text NOT NULL,
	`url` text NOT NULL,
	`created_by` text NOT NULL,
	`created_at` text NOT NULL
);
--> statement-breakpoint
CREATE TABLE `users` (
	`github_user_id` integer PRIMARY KEY,
	`github_username` text NOT NULL,
	`created_at` text NOT NULL
);
