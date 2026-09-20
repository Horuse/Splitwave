const tasks = [
	{
		name: 'frontend',
		command: ['bun', 'run', 'check:frontend']
	},
	{
		name: 'backend',
		command: ['cargo', 'check', '--manifest-path', 'src-tauri/Cargo.toml']
	}
] as const;

const children = tasks.map((task) => ({
	...task,
	process: Bun.spawn(task.command, {
		stdin: 'inherit',
		stdout: 'inherit',
		stderr: 'inherit'
	})
}));

const results = await Promise.all(children.map(async ({ name, process }) => ({ name, exitCode: await process.exited })));
const failed = results.filter(({ exitCode }) => exitCode !== 0);

if (failed.length > 0) {
	for (const { name, exitCode } of failed) {
		console.error(`${name} check failed with exit code ${exitCode}`);
	}
	process.exit(1);
}
