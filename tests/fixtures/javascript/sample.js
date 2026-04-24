import { formatName as format } from "./shared";

export class UserService {
    run(user) {
        return format(user.name);
    }
}

export function run(user) {
    return format(user.name);
}
