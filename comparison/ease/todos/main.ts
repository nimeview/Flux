type Filter = "All" | "Active" | "Done";

const shows = (f: Filter, done: boolean) => f === "All" || (f === "Active" ? !done : done);

class Item {
  done = false;
  title: string;
  constructor(title: string) { this.title = title; }
  toggle() { this.done = !this.done; }
  get line() { return `${this.done ? "[x]" : "[ ]"} ${this.title}`; }
}

class TodoList {
  items: Item[] = [];
  filter: Filter = "All";
  add(text: string) { this.items.push(new Item(text)); }
  remove(index: number) { const gone = this.items[index]; this.items = this.items.filter((x) => x !== gone); }
  show(f: Filter) { this.filter = f; }
  get visible() { return this.items.filter((i) => shows(this.filter, i.done)); }
  get left() { return this.items.filter((i) => !i.done).length; }
}

function render(todos: TodoList, step: string) {
  console.log(`-- ${step} (filter: Filter::${todos.filter})`);
  for (const item of todos.visible) console.log(`  ${item.line}`);
  console.log(`  ${todos.left} item${todos.left === 1 ? "" : "s"} left`);
}

const todos = new TodoList();
todos.add("write code");
todos.add("write tests");
todos.add("ship it");
render(todos, "added three");
todos.items[0].toggle();
render(todos, "toggled #0");
todos.show("Active");
render(todos, "show active");
todos.items[1].toggle();
render(todos, "toggled #1");
todos.show("Done");
render(todos, "show done");
todos.remove(0);
render(todos, "removed #0");
todos.show("All");
todos.items[0].toggle();
render(todos, "show all, toggled #0 back");
