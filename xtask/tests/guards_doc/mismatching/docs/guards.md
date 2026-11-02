# Guards

```yaml guard=sample
rules:
  dependencies:
    forbidden:
      - name: no-sample
        comment: "A sample rule. plan:0005-guard-catalogue"
        severity: warm
        from: {}
        to: { circular: true }
      - name: other
        from: {}
        to: { circular: true }
```
