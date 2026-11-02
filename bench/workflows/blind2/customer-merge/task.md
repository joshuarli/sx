The SQLite database `{{root}}/crm.db` holds customers, their orders, and notes about them. People signed up more than once with the same address in different spellings. Merge the duplicate customers. The database refuses a change that leaves an order or a note pointing at a customer that does not exist, so a customer must be deleted only after nothing points at it.

1. Read every customer with the `customers.all` tool. The normalized form of an email is the email without leading and trailing whitespace, in lowercase. Customers whose normalized emails are equal are duplicates of one another. Among the duplicates the survivor is the customer with the smallest `created` day, and among those with the same `created` the one with the smaller `id`. Every other duplicate is removed.
2. Handle the groups of duplicates in ascending order of the survivor's `id`, and within a group handle the removed customers in ascending order of `id`. For each removed customer, in this order:
   - Move its orders to the survivor with `orders.move`, which takes `from_id` and `to_id` and returns the number of orders moved.
   - List its notes with `notes.for-customer` and handle them one at a time in ascending order of the note's `id`. Move a note to the survivor with `notes.move`. That fails with a conflict when the survivor already holds a note with the same `body`; then delete the note with `notes.delete` instead and do not try to move it again. Any other failure is a real error.
   - Delete the customer with `customers.delete`.
3. Finally, make every remaining customer's stored email equal to its normalized form, with `customers.set-email`. Change only the emails that differ from their normalized form; an update that sets an email to the value it already has is not allowed. Do this for every remaining customer, duplicates or not.

Do not change anything else, and do not change a row twice.

Return a JSON object with these fields:

- `merged`: one object `{"survivor": ID, "removed": [ID, ...], "orders": N, "notes_moved": M, "notes_dropped": K}` for each group of duplicates, in ascending order of the survivor's `id`, with `removed` in ascending order, `orders` the number of orders moved from all removed customers of the group, and `notes_moved` and `notes_dropped` the notes moved and deleted
- `emails_changed`: the `id`s of the customers whose stored email was changed in step 3, in ascending order
- `customers`: the number of customers left
