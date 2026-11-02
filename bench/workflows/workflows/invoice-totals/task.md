The directory `{{root}}/invoices` holds one invoice per file, files named `NAME.csv`. Total the invoices per customer and find the invoices whose stated total is wrong. Ignore files whose names do not end in `.csv`.

An invoice file is text with lines ending in `\n`; blank lines are to be ignored. The first line is `invoice,ID,CUSTOMER,STATED_TOTAL`. Every other line is an invoice line, `SKU,QUANTITY,UNIT_PRICE,DISCOUNT_PERCENT`. `QUANTITY` is a positive whole number. `UNIT_PRICE` and `DISCOUNT_PERCENT` are non-negative decimal numbers written with digits and at most one `.`, such as `19.99`, `0.125`, or `12.5`. `STATED_TOTAL` is a decimal number with at most 2 decimal places. No field contains a comma.

Money must be exact, with no binary floating point. For each invoice line, the line amount is `QUANTITY × UNIT_PRICE × (100 − DISCOUNT_PERCENT) ÷ 100`, computed exactly and then rounded to 2 decimal places, with an exact half rounded away from zero (so `0.125` becomes `0.13` and `0.124` becomes `0.12`). The computed total of an invoice is the sum of its rounded line amounts. An invoice is mismatched when its computed total is not equal in value to its stated total (`125.4` equals `125.40`).

A customer's total is the sum of the computed totals of all that customer's invoices, whether or not they are mismatched. Customers are told apart by the exact text of `CUSTOMER`.

Return a JSON object with two fields:

- `customers`: one object `{"customer": CUSTOMER, "invoices": COUNT, "total": TOTAL}` per customer, in ascending order of customer name, with `COUNT` the number of that customer's invoices
- `mismatched`: one object `{"invoice": ID, "stated": STATED, "computed": COMPUTED}` per mismatched invoice, in ascending order of `ID`

Every money value in the result (`TOTAL`, `STATED`, `COMPUTED`) is a JSON string with exactly 2 decimal places, such as `"1234.50"` or `"0.00"`.
