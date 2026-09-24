using System;

namespace Sample;

public interface IEntity
{
}

[AttributeUsage(AttributeTargets.Class)]
public sealed class AuditAttribute : Attribute
{
    public AuditAttribute(string area)
    {
        Area = area;
    }

    public string Area { get; }

    public int Level { get; set; }
}

[Audit("shared"), Serializable]
internal sealed class Marker
{
    [Obsolete("use the other")]
    public Type Kind = typeof(IEntity);
}
