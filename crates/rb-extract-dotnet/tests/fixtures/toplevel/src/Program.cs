using System.Threading.Tasks;
using TopLevel.Clock;

var clock = new SystemClock();
await Task.Yield();
System.Console.WriteLine(clock.Now().Describe());
