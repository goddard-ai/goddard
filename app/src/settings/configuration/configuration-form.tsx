/* oxlint-disable tsrx/prefer-oninput -- RJSF exposes form updates through an onChange component prop. */
import type { UserConfigDocument } from "@goddard-ai/sdk"
import { t } from "@lingui/core/macro"
import Form from "@rjsf/core"
import {
  ariaDescribedByIds,
  buttonId,
  canExpand,
  descriptionId,
  getUiOptions,
  replaceStringParameters,
  titleId,
  TranslatableString,
  type BaseInputTemplateProps,
  type ErrorSchema,
  type IconButtonProps,
  type ObjectFieldTemplatePropertyType,
  type ObjectFieldTemplateProps,
  type OptionalDataControlsTemplateProps,
  type RJSFSchema,
  type TemplatesType,
  type UiSchema,
} from "@rjsf/utils"
import { customizeValidator } from "@rjsf/validator-ajv8"
import Ajv2020 from "ajv/dist/2020.js"
import { ChevronDown, ChevronUp, Copy, Plus, Trash2, X, type LucideIcon } from "lucide-react"
import type { ComponentChildren } from "preact"
import { useEffect, useRef, useState } from "preact/hooks"

const validator = customizeValidator({ AjvClass: Ajv2020 })

const uiSchema = {
  "ui:globalOptions": {
    enableOptionalDataFieldForType: ["array"],
  },
  "ui:options": {
    enableOptionalDataFieldForType: [],
  },
  "ui:submitButtonOptions": {
    norender: true,
  },
} satisfies UiSchema<UserConfigDocument>

const defaultFormStateBehavior = {
  allOf: "skipDefaults",
  arrayMinItems: { populate: "never" },
  constAsDefaults: "never",
  emptyObjectFields: "skipDefaults",
  mergeDefaultsIntoFormData: "useFormDataIfPresent",
} as const

const buttonTemplates = {
  AddButton: (props: IconButtonProps) => (
    <SchemaIconButton {...props} fallbackLabel={t`Add`} Icon={Plus} />
  ),
  ClearButton: (props: IconButtonProps) => (
    <SchemaIconButton {...props} fallbackLabel={t`Clear`} Icon={X} />
  ),
  CopyButton: (props: IconButtonProps) => (
    <SchemaIconButton {...props} fallbackLabel={t`Copy`} Icon={Copy} />
  ),
  MoveDownButton: (props: IconButtonProps) => (
    <SchemaIconButton {...props} fallbackLabel={t`Move down`} Icon={ChevronDown} />
  ),
  MoveUpButton: (props: IconButtonProps) => (
    <SchemaIconButton {...props} fallbackLabel={t`Move up`} Icon={ChevronUp} />
  ),
  RemoveButton: (props: IconButtonProps) => (
    <SchemaIconButton {...props} fallbackLabel={t`Remove`} Icon={Trash2} />
  ),
  SubmitButton: () => null,
}

const templates = {
  BaseInputTemplate: CommitOnBlurInput,
  ButtonTemplates: buttonTemplates,
  ObjectFieldTemplate: CollapsibleObjectFieldTemplate,
  OptionalDataControlsTemplate,
} satisfies Partial<TemplatesType>

type ConfigurationFormProps = {
  disabled: boolean
  document: UserConfigDocument
  errors?: ErrorSchema<UserConfigDocument>
  onDocumentChange: (document: UserConfigDocument) => void
  schema: RJSFSchema
}

/** Renders the daemon-provided JSON Schema without maintaining a parallel field catalog. */
export function ConfigurationForm(props: ConfigurationFormProps) {
  return (
    <Form<UserConfigDocument>
      disabled={props.disabled}
      experimental_defaultFormStateBehavior={defaultFormStateBehavior}
      extraErrors={props.errors}
      formData={props.document}
      noHtml5Validate={true}
      schema={props.schema}
      showErrorList={false}
      templates={templates}
      translateString={translateSchemaString}
      uiSchema={uiSchema}
      validator={validator}
      onChange={({ formData }) => {
        props.onDocumentChange(formData ?? {})
      }}
    />
  )
}

function CollapsibleObjectFieldTemplate(props: ObjectFieldTemplateProps) {
  const {
    className,
    description,
    disabled,
    errorSchema,
    fieldPathId,
    formData,
    onAddProperty,
    properties,
    readonly,
    registry,
    required,
    schema,
    title,
    uiSchema,
  } = props
  const isRoot = fieldPathId.$id === "root"
  const isPureUnion =
    (schema.oneOf || schema.anyOf) && !schema.properties && properties.length === 0
  if (isPureUnion) {
    return null
  }

  const content = (
    <>
      {description && (
        <div className="field-description" id={descriptionId(fieldPathId)}>
          {description}
        </div>
      )}
      {properties.map((property: ObjectFieldTemplatePropertyType) => property.content)}
      {canExpand(schema, uiSchema, formData) && (
        <registry.templates.ButtonTemplates.AddButton
          className="rjsf-object-property-expand"
          disabled={disabled || readonly}
          id={buttonId(fieldPathId, "add")}
          registry={registry}
          uiSchema={uiSchema}
          onClick={onAddProperty}
        />
      )}
    </>
  )

  if (isRoot) {
    return (
      <fieldset className={className} id={fieldPathId.$id}>
        {content}
      </fieldset>
    )
  }

  const hasData =
    typeof formData === "object" && formData !== null && Object.keys(formData).length > 0
  const hasErrors = Boolean(errorSchema && Object.keys(errorSchema).length > 0)
  const options = getUiOptions(uiSchema)

  return (
    <CollapsibleObjectDetails
      className={className}
      content={content}
      hasErrors={hasErrors}
      initiallyOpen={hasData}
      summaryId={titleId(fieldPathId)}
      title={
        <>
          {options.title ?? schema.title ?? title}
          {required ? "*" : null}
        </>
      }
    />
  )
}

function CollapsibleObjectDetails(props: {
  className?: string
  content: ComponentChildren
  hasErrors: boolean
  initiallyOpen: boolean
  summaryId: string
  title: ComponentChildren
}) {
  const [open, setOpen] = useState(props.initiallyOpen || props.hasErrors)

  useEffect(() => {
    if (props.hasErrors) {
      setOpen(true)
    }
  }, [props.hasErrors])

  return (
    <details
      className={`${props.className ?? ""} rjsf-object-details`}
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary id={props.summaryId}>{props.title}</summary>
      <div className="rjsf-object-details-content">{props.content}</div>
    </details>
  )
}

function CommitOnBlurInput(props: BaseInputTemplateProps) {
  const nextExternalValue = formatInputValue(props.value, props.type)
  const [draft, setDraft] = useState(nextExternalValue)
  const lastCommittedDraft = useRef(nextExternalValue)

  useEffect(() => {
    setDraft(nextExternalValue)
    lastCommittedDraft.current = nextExternalValue
  }, [nextExternalValue])

  const commit = () => {
    if (draft === lastCommittedDraft.current) {
      return
    }

    lastCommittedDraft.current = draft
    props.onChange(draft === "" ? props.options.emptyValue : draft, undefined, props.id)
  }
  const inputType = resolveInputType(props.type, props.schema)

  return (
    <input
      aria-describedby={ariaDescribedByIds(props.id)}
      aria-invalid={props.rawErrors && props.rawErrors.length > 0 ? "true" : undefined}
      autoFocus={props.autofocus}
      className="form-control"
      disabled={props.disabled}
      id={props.id}
      max={props.schema.maximum}
      maxLength={props.schema.maxLength}
      min={props.schema.minimum}
      minLength={props.schema.minLength}
      name={props.htmlName || props.id}
      pattern={props.schema.pattern}
      placeholder={props.placeholder}
      readOnly={props.readonly}
      required={props.required}
      step={
        props.schema.multipleOf ??
        (props.schema.type === "integer" || props.type === "integer" ? 1 : undefined)
      }
      type={inputType as "text"}
      value={draft}
      onBlur={() => {
        commit()
        props.onBlur(props.id, draft)
      }}
      onFocus={() => {
        props.onFocus(props.id, draft)
      }}
      onInput={(event) => {
        setDraft(event.currentTarget.value)
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault()
          commit()
        }
      }}
    />
  )
}

function SchemaIconButton(
  props: IconButtonProps & {
    fallbackLabel: string
    Icon: LucideIcon
  },
) {
  const {
    fallbackLabel,
    Icon,
    icon: _icon,
    iconType: _iconType,
    registry: _registry,
    uiSchema: _uiSchema,
    ...buttonProps
  } = props
  const label = buttonProps["aria-label"] ?? buttonProps.title ?? fallbackLabel

  return (
    <button {...buttonProps} aria-label={label} title={buttonProps.title ?? label} type="button">
      <Icon aria-hidden={true} size={14} strokeWidth={2.2} />
    </button>
  )
}

function OptionalDataControlsTemplate(props: OptionalDataControlsTemplateProps) {
  const action = props.onAddClick ?? props.onRemoveClick
  if (!action) {
    return <span id={props.id}>{props.label}</span>
  }

  const Icon = props.onAddClick ? Plus : X
  return (
    <button
      aria-label={props.label}
      className="rjsf-optional-data-control"
      id={props.id}
      title={props.label}
      type="button"
      onClick={action}
    >
      <Icon aria-hidden={true} size={14} strokeWidth={2.2} />
    </button>
  )
}

function formatInputValue(value: unknown, type: string) {
  if (type === "number" || type === "integer") {
    return value || value === 0 ? String(value) : ""
  }
  return value == null ? "" : String(value)
}

function resolveInputType(type: string | undefined, schema: RJSFSchema) {
  if (
    type === "number" ||
    type === "integer" ||
    schema.type === "number" ||
    schema.type === "integer"
  ) {
    return "number"
  }
  if (["date", "datetime-local", "email", "password", "time", "url"].includes(type ?? "")) {
    return type
  }
  return "text"
}

function translateSchemaString(key: TranslatableString, params: string[] = []) {
  switch (key) {
    case TranslatableString.ArrayItemTitle:
      return t`Item`
    case TranslatableString.MissingItems:
      return t`Missing items definition`
    case TranslatableString.EmptyArray:
      return t`No items yet.`
    case TranslatableString.YesLabel:
      return t`Yes`
    case TranslatableString.NoLabel:
      return t`No`
    case TranslatableString.CloseLabel:
      return t`Close`
    case TranslatableString.ErrorsLabel:
      return t`Errors`
    case TranslatableString.NewStringDefault:
      return t`New value`
    case TranslatableString.AddButton:
      return t`Add`
    case TranslatableString.AddItemButton:
      return t`Add item`
    case TranslatableString.CopyButton:
      return t`Copy`
    case TranslatableString.MoveDownButton:
      return t`Move down`
    case TranslatableString.MoveUpButton:
      return t`Move up`
    case TranslatableString.RemoveButton:
      return t`Remove`
    case TranslatableString.NowLabel:
      return t`Now`
    case TranslatableString.ClearLabel:
    case TranslatableString.ClearButton:
      return t`Clear`
    case TranslatableString.AriaDateLabel:
      return t`Select a date`
    case TranslatableString.PreviewLabel:
      return t`Preview`
    case TranslatableString.DecrementAriaLabel:
      return t`Decrease value by 1`
    case TranslatableString.IncrementAriaLabel:
      return t`Increase value by 1`
    case TranslatableString.OptionalObjectAdd:
      return t`Add optional setting`
    case TranslatableString.OptionalObjectRemove:
      return t`Remove optional setting`
    case TranslatableString.OptionalObjectEmptyMsg:
      return t`No value for this optional setting`
    case TranslatableString.Type:
      return t`Type`
    case TranslatableString.Value:
      return t`Value`
    case TranslatableString.UnknownFieldType:
      return t`Unknown field type ${params[0] ?? ""}`
    case TranslatableString.OptionPrefix:
      return t`Option ${params[0] ?? ""}`
    case TranslatableString.TitleOptionPrefix:
      return t`${params[0] ?? ""} option ${params[1] ?? ""}`
    case TranslatableString.KeyLabel:
      return t`${params[0] ?? ""} key`
    case TranslatableString.DeprecatedLabel:
      return t`${params[0] ?? ""} (deprecated)`
    case TranslatableString.InvalidObjectField:
      return t`Invalid object field configuration.`
    case TranslatableString.UnsupportedField:
    case TranslatableString.UnsupportedFieldWithId:
    case TranslatableString.UnsupportedFieldWithReason:
    case TranslatableString.UnsupportedFieldWithIdAndReason:
      return t`Unsupported field schema.`
    case TranslatableString.FilesInfo:
      return t`${params[0] ?? ""} (${params[1] ?? ""}, ${params[2] ?? ""} bytes)`
    default:
      return replaceStringParameters(key, params)
  }
}
