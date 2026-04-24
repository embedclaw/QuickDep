import { formatName as format } from "./shared";

export interface Greeter {
  greet(name: string): string;
}

export class UserService implements Greeter {
  public greet(name: string): string {
    return formatName(name);
  }
}

function formatName(name: string): string {
  return name.trim();
}

export const run = (name: string): string => {
  return format(name);
};
