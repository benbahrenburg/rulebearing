// Copied from ArchUnitNET 0.13.4, ArchUnitNETTests/Fluent/Slices/SlicesTests.cs, the first test
// (CycleDetectionTest) only. ArchUnitNET's authors (its NOTICE file); software developed at TNG Technology
// Consulting GmbH. Apache License 2.0
// (https://www.apache.org/licenses/LICENSE-2.0). A fixture for rulebearing import archunit.
using System.Linq;
using ArchUnitNET.Fluent.Slices;
using ArchUnitNET.xUnit;
using Xunit;

namespace ArchUnitNETTests.Fluent.Slices
{
    public class SlicesTests
    {
        [Fact]
        public void CycleDetectionTest()
        {
            Assert.Throws<FailedArchRuleException>(() =>
                SliceRuleDefinition
                    .Slices()
                    .Matching("TestAssembly.Slices.(**)")
                    .Should()
                    .BeFreeOfCycles()
                    .Check(StaticTestArchitectures.ArchUnitNETTestAssemblyArchitecture)
            );
            Assert.False(
                SliceRuleDefinition
                    .Slices()
                    .Matching("TestAssembly.Slices.(**)")
                    .Should()
                    .BeFreeOfCycles()
                    .HasNoViolations(StaticTestArchitectures.ArchUnitNETTestAssemblyArchitecture)
            );
            Assert.True(
                SliceRuleDefinition
                    .Slices()
                    .Matching("TestAssembly.Slices.(**)..")
                    .Should()
                    .BeFreeOfCycles()
                    .HasNoViolations(StaticTestArchitectures.ArchUnitNETTestAssemblyArchitecture)
            );
        }
    }
}
