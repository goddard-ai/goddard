# Forms

## Field Context

Nest Ark form components inside `Field.Root` to inherit `disabled`, `invalid`, `required`, and `readOnly` automatically.

```tsx
import { Field } from "@ark-ui/react/field";
import { NumberInput } from "@ark-ui/react/number-input";

const Demo = () => (
  <Field.Root disabled>
    <NumberInput.Root /> {/* NumberInput will be disabled */}
  </Field.Root>
);
```

## Labels, Helper Text, and Errors

- Use `Field.Label` to provide an accessible label.
- Use `Field.HelperText` to provide extra guidance.
- Use `Field.ErrorText` to provide validation feedback.
- Keep those elements close to the input they describe so the generated ARIA relationships stay obvious.

When the input is invalid, pass `invalid` to `Field.Root` and render a matching `Field.ErrorText`.

```tsx
<Field.Root invalid>
  <Field.Label>Username</Field.Label>
  <Field.Input placeholder="Enter your username" />
  <Field.ErrorText>Username is required.</Field.ErrorText>
</Field.Root>
```

## React Hook Form

Use `register` for simple inputs and reach for `Controller` when a component needs a more custom value interface.

```tsx
import { Field } from "@ark-ui/react/field";
import { useForm } from "react-hook-form";

const Demo = () => {
  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<FormValues>();

  return (
    <form onSubmit={handleSubmit(onSubmit)}>
      <Field.Root invalid={!!errors.firstName}>
        <Field.Label>First Name</Field.Label>
        <Field.Input
          {...register("firstName", { required: "First name is required" })}
        />
        <Field.ErrorText>{errors.firstName?.message}</Field.ErrorText>
      </Field.Root>
      <button type="submit">Submit</button>
    </form>
  );
};
```

## Fieldset Context

Use `Fieldset` when multiple related inputs should share one grouping or when a composite component renders multiple `input` elements.
