// The compiler support types netstandard2.0 lacks, for records, init-only and required members.
// Internal, so nothing outside the analyzer sees them.

#pragma warning disable IDE0130 // The namespaces are the compiler's, not this folder's.
namespace System.Runtime.CompilerServices
{
    /// <summary>Marks init-only setters.</summary>
    internal static class IsExternalInit
    {
    }

    /// <summary>Marks required members.</summary>
    [AttributeUsage(AttributeTargets.Class | AttributeTargets.Struct | AttributeTargets.Field | AttributeTargets.Property, Inherited = false)]
    internal sealed class RequiredMemberAttribute : Attribute
    {
    }

    /// <summary>Names a compiler feature a member needs.</summary>
    [AttributeUsage(AttributeTargets.All, AllowMultiple = true, Inherited = false)]
    internal sealed class CompilerFeatureRequiredAttribute : Attribute
    {
        /// <summary>The feature <paramref name="featureName"/>.</summary>
        public CompilerFeatureRequiredAttribute(string featureName) => FeatureName = featureName;

        /// <summary>The feature's name.</summary>
        public string FeatureName { get; }
    }
}

namespace System.Diagnostics.CodeAnalysis
{
    /// <summary>Marks a constructor that sets every required member.</summary>
    [AttributeUsage(AttributeTargets.Constructor, Inherited = false)]
    internal sealed class SetsRequiredMembersAttribute : Attribute
    {
    }
}
