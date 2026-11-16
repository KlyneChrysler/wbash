import { readFile } from "node:fs/promises";
import { join } from "node:path";

interface User {
  id: string;
  email: string;
  roles: string[];
}

type Result<T> = { ok: true; value: T } | { ok: false; error: string };

export async function loadUsers(root: string): Promise<Result<User[]>> {
  try {
    const text = await readFile(join(root, "users.json"), "utf8");
    const parsed = JSON.parse(text) as unknown;
    if (!Array.isArray(parsed)) {
      return { ok: false, error: "expected an array" };
    }
    return { ok: true, value: parsed as User[] };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

export function hasRole(user: User, role: string): boolean {
  return user.roles.includes(role);
}

export const byEmail = (a: User, b: User): number => a.email.localeCompare(b.email);
