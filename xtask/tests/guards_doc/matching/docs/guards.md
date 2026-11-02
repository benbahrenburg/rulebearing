# Guards

```yaml guard=sample
rules:
  dependencies:
    forbidden:
      - name: no-sample
        comment: "A sample rule. plan:0005-guard-catalogue"
        severity: warn
        from: {}
        to: { circular: true }
      - name: other
        from: {}
        to: { circular: true }
```

One rule of it:

```yaml guard=sample#no-sample
      - name: no-sample
        comment: "A sample rule. plan:0005-guard-catalogue"
        severity: warn
        from: {}
        to: { circular: true }
```

```sh
not a recipe
```
